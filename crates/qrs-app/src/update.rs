//! The updater (P20.1, D36): new in quickRS (the Python app had none).
//!
//! On start (at most once a day, `update_check` in config.json turns it off) and from Settings > Updates, a
//! background thread asks GitHub for the public repo's latest release and compares its version with this one.
//! A newer one shows a small notice (Update now / Later / Skip this version). Update now downloads this OS's zip and
//! `SHA256SUMS.txt` from the release, refuses the zip unless its SHA-256 is the one listed, unpacks it into
//! `<install>/.update/<version>/` and swaps it in: each file in place is renamed to `<name>.old` (Windows allows
//! renaming a running exe, not overwriting it), the new one moved in, the app started again and closed; the next
//! start deletes the `.old` files and the staging folder. Files that aren't in the zip are never touched, and the
//! user's own (keys, config.json, userdata.json, dot folders) are skipped even if a zip had them. On macOS the whole
//! `quickRS.app` is swapped the same way (as one entry, so the bundle stays as it was signed). An install folder
//! that can't be written to opens the release page in the browser instead.
//!
//! Release assets (the kit, `../quickRS-public`, makes them; `quickrs-cli sums` writes the checksums):
//!     quickRS-<ver>-windows-x64.zip      one folder `quickRS-<ver>/` with the programs (or the files at the root)
//!     quickRS-<ver>-macos-universal.zip  `quickRS.app` (at the root or in one folder)
//!     SHA256SUMS.txt                     `<sha256 hex>  <file name>` per line (sha256sum's format)
//!
//! `QUICKRS_UPDATE_URL` points the check at another release JSON (another repo's API URL, or a local file: tests);
//! `QUICKRS_UPDATE_AS` pretends this is another version.

use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use qrs_library::Config;

/// The public repo's latest release (GitHub's API).
pub const LATEST_API: &str = "https://api.github.com/repos/arpan2711/quickRS-player/releases/latest";
/// Its page, for when the app can't update itself.
pub const RELEASES_PAGE: &str = "https://github.com/arpan2711/quickRS-player/releases/latest";
pub const SUMS_NAME: &str = "SHA256SUMS.txt";
/// Automatic checks at most this often.
const CHECK_EVERY_S: u64 = 24 * 3600;
/// No asset is bigger than this (a guard against a runaway download).
const MAX_DOWNLOAD: u64 = 512 * 1024 * 1024;
/// The staging folder in the install folder.
const STAGING: &str = ".update";
/// Lists the `.old` files a swap left behind (deleted on the next start).
const LEFTOVERS: &str = "leftovers.txt";

// -- config.json keys (new keys; the Python app ignores them) --------------------------------------------------------
/// Check on start (bool, default true).
pub const KEY_CHECK: &str = "update_check";
/// When the last automatic check ran (epoch seconds).
pub const KEY_LAST: &str = "update_last_check";
/// A version the user said to skip ("0.3.0").
pub const KEY_SKIP: &str = "update_skip";

// -- versions ---------------------------------------------------------------------------------------------------------

/// A semantic version: `1.2.3`, `v1.2.3`, `1.2` (patch 0), `1.2.3-beta.1` (a pre-release, older than `1.2.3`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Version {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
    pub pre: Vec<String>,
}

impl Version {
    pub fn parse(s: &str) -> Option<Version> {
        let s = s.trim();
        let s = s.strip_prefix(['v', 'V']).unwrap_or(s);
        let s = s.split('+').next().unwrap_or(s); // (build metadata doesn't count)
        let (core, pre) = match s.split_once('-') {
            Some((c, p)) => (c, p.split('.').map(str::to_string).collect()),
            None => (s, Vec::new()),
        };
        let mut nums = core.split('.').map(|n| n.parse::<u64>());
        let major = nums.next()?.ok()?;
        let minor = nums.next().unwrap_or(Ok(0)).ok()?;
        let patch = nums.next().unwrap_or(Ok(0)).ok()?;
        if nums.next().is_some() {
            return None;
        }
        Some(Version {
            major,
            minor,
            patch,
            pre,
        })
    }

    /// The running program's version.
    pub fn current() -> Version {
        std::env::var("QUICKRS_UPDATE_AS")
            .ok()
            .and_then(|v| Version::parse(&v))
            .unwrap_or_else(|| Version::parse(env!("CARGO_PKG_VERSION")).expect("the crate's version"))
    }
}

impl Ord for Version {
    fn cmp(&self, o: &Self) -> std::cmp::Ordering {
        use std::cmp::Ordering;
        (self.major, self.minor, self.patch)
            .cmp(&(o.major, o.minor, o.patch))
            .then_with(|| match (self.pre.is_empty(), o.pre.is_empty()) {
                (true, true) => Ordering::Equal,
                (true, false) => Ordering::Greater,
                (false, true) => Ordering::Less,
                (false, false) => {
                    // semver: identifiers left to right, numbers below words, numbers by value
                    for (a, b) in self.pre.iter().zip(&o.pre) {
                        let c = match (a.parse::<u64>(), b.parse::<u64>()) {
                            (Ok(x), Ok(y)) => x.cmp(&y),
                            (Ok(_), Err(_)) => Ordering::Less,
                            (Err(_), Ok(_)) => Ordering::Greater,
                            (Err(_), Err(_)) => a.cmp(b),
                        };
                        if c != Ordering::Equal {
                            return c;
                        }
                    }
                    self.pre.len().cmp(&o.pre.len())
                }
            })
    }
}

impl PartialOrd for Version {
    fn partial_cmp(&self, o: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(o))
    }
}

impl std::fmt::Display for Version {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)?;
        if !self.pre.is_empty() {
            write!(f, "-{}", self.pre.join("."))?;
        }
        Ok(())
    }
}

// -- releases ---------------------------------------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
pub struct Asset {
    pub name: String,
    pub url: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Release {
    pub version: Version,
    /// The release's page (its notes).
    pub page: String,
    pub assets: Vec<Asset>,
}

/// GitHub's release JSON (`tag_name`, `html_url`, `assets[].name`, `assets[].browser_download_url`). Drafts and
/// pre-releases are not offered.
pub fn parse_release(json: &str) -> Result<Release, String> {
    let v: serde_json::Value = serde_json::from_str(json).map_err(|e| format!("not a release: {e}"))?;
    if v.get("draft").and_then(|d| d.as_bool()) == Some(true)
        || v.get("prerelease").and_then(|d| d.as_bool()) == Some(true)
    {
        return Err("the latest release is a draft or pre-release".into());
    }
    let tag = v
        .get("tag_name")
        .and_then(|t| t.as_str())
        .ok_or("the release has no tag")?;
    let version = Version::parse(tag).ok_or_else(|| format!("the release's tag {tag:?} isn't a version"))?;
    let page = v
        .get("html_url")
        .and_then(|t| t.as_str())
        .unwrap_or(RELEASES_PAGE)
        .to_string();
    let assets = v
        .get("assets")
        .and_then(|a| a.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|x| {
                    Some(Asset {
                        name: x.get("name")?.as_str()?.to_string(),
                        url: x.get("browser_download_url")?.as_str()?.to_string(),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    Ok(Release {
        version,
        page,
        assets,
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Os {
    Windows,
    Mac,
    Other,
}

impl Os {
    pub fn this() -> Os {
        if cfg!(windows) {
            Os::Windows
        } else if cfg!(target_os = "macos") {
            Os::Mac
        } else {
            Os::Other
        }
    }
}

/// The zip a release has for this OS: `quickRS-<ver>-windows-x64.zip`, `quickRS-<ver>-macos-universal.zip`.
pub fn asset_name(version: &Version, os: Os) -> Option<String> {
    match os {
        Os::Windows => Some(format!("quickRS-{version}-windows-x64.zip")),
        Os::Mac => Some(format!("quickRS-{version}-macos-universal.zip")),
        Os::Other => None,
    }
}

pub fn pick_asset(rel: &Release, os: Os) -> Option<&Asset> {
    let want = asset_name(&rel.version, os)?;
    rel.assets.iter().find(|a| a.name.eq_ignore_ascii_case(&want))
}

/// `SHA256SUMS.txt` (sha256sum's output: `<64 hex>  <name>`, `*<name>` in binary mode; `#` comments and blank lines
/// skipped): file name to lower-case hex.
pub fn parse_sums(text: &str) -> HashMap<String, String> {
    let mut out = HashMap::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((hash, name)) = line.split_once(char::is_whitespace) else {
            continue;
        };
        let name = name.trim_start();
        let name = name.strip_prefix('*').unwrap_or(name).trim();
        if hash.len() == 64 && hash.bytes().all(|b| b.is_ascii_hexdigit()) && !name.is_empty() {
            out.insert(name.to_string(), hash.to_ascii_lowercase());
        }
    }
    out
}

/// The `SHA256SUMS.txt` line for a file (what `quickrs-cli sums` writes).
#[cfg(test)]
pub fn sums_line(name: &str, bytes: &[u8]) -> String {
    format!("{}  {name}", sha256_hex(bytes))
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

/// The download is the file the release lists, or it's refused.
pub fn verify(name: &str, bytes: &[u8], sums: &HashMap<String, String>) -> Result<(), String> {
    let want = sums
        .get(name)
        .ok_or_else(|| format!("{SUMS_NAME} doesn't list {name}"))?;
    let got = sha256_hex(bytes);
    if &got != want {
        return Err(format!(
            "{name} isn't the file the release lists (its checksum doesn't match); nothing was changed"
        ));
    }
    Ok(())
}

// -- unpacking and swapping -----------------------------------------------------------------------------------------

/// Unpack a zip into `dir` (emptied first). Entries that would land outside it are refused.
pub fn extract_zip(bytes: &[u8], dir: &Path) -> Result<(), String> {
    if dir.exists() {
        std::fs::remove_dir_all(dir).map_err(|e| format!("couldn't clear {}: {e}", dir.display()))?;
    }
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).map_err(|e| format!("not a zip: {e}"))?;
    for i in 0..zip.len() {
        let mut f = zip.by_index(i).map_err(|e| e.to_string())?;
        let rel = f
            .enclosed_name()
            .ok_or_else(|| format!("the zip has an unsafe path: {}", f.name_raw().escape_ascii()))?;
        let out = dir.join(rel);
        if f.is_dir() {
            std::fs::create_dir_all(&out).map_err(|e| e.to_string())?;
            continue;
        }
        if let Some(parent) = out.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let mut buf = Vec::with_capacity(f.size() as usize);
        f.read_to_end(&mut buf).map_err(|e| e.to_string())?;
        std::fs::write(&out, &buf).map_err(|e| e.to_string())?;
        #[cfg(unix)]
        if let Some(mode) = f.unix_mode() {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&out, std::fs::Permissions::from_mode(mode & 0o777));
        }
    }
    Ok(())
}

/// What in an unpacked zip goes into the install folder: Windows, the one folder the zip holds (or its root);
/// macOS, the folder holding `quickRS.app` (the root, or one folder down).
pub fn staged_root(dir: &Path, os: Os) -> Option<PathBuf> {
    let entries: Vec<PathBuf> = std::fs::read_dir(dir)
        .ok()?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .collect();
    match os {
        Os::Mac => {
            let has_app = |d: &Path| {
                std::fs::read_dir(d).ok().is_some_and(|rd| {
                    rd.filter_map(|e| e.ok())
                        .any(|e| e.path().is_dir() && is_app(&e.path()))
                })
            };
            if has_app(dir) {
                return Some(dir.to_path_buf());
            }
            entries.into_iter().find(|e| e.is_dir() && has_app(e))
        }
        _ => {
            if entries.len() == 1 && entries[0].is_dir() {
                Some(entries[0].clone())
            } else if entries.is_empty() {
                None
            } else {
                Some(dir.to_path_buf())
            }
        }
    }
}

fn is_app(p: &Path) -> bool {
    p.extension().is_some_and(|e| e.eq_ignore_ascii_case("app"))
}

/// The user's own files and folders: never replaced, whatever a zip holds (keys, settings, scores, licks; the data
/// folders, which all start with a dot: `.quickrs-cache`, `.update`).
pub fn protected(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    n.starts_with('.')
        || matches!(
            n.as_str(),
            "keys.json" | "keys.json.txt" | "config.json" | "userdata.json" | "lessons" | "takes"
        )
        || n.ends_with(".old")
}

/// Swap what's in `staged` into `target`: each file there renamed to `<name>.old`, the new one moved in. With
/// `whole_dirs` a folder at the top (macOS's `quickRS.app`) is swapped as one entry; otherwise folders are merged
/// file by file. On a failure everything done is put back. Returns the `.old` files left (still in use; deleted by
/// `cleanup` on the next start).
pub fn swap_in(staged: &Path, target: &Path, whole_dirs: bool) -> Result<Vec<PathBuf>, String> {
    let mut done: Vec<(PathBuf, Option<PathBuf>)> = Vec::new();
    let res = swap_dir(staged, target, whole_dirs, &mut done);
    if let Err(e) = res {
        for (new, old) in done.into_iter().rev() {
            let _ = remove_any(&new);
            if let Some(old) = old {
                let _ = std::fs::rename(&old, &new);
            }
        }
        return Err(e);
    }
    let mut left = Vec::new();
    for (_, old) in done {
        if let Some(old) = old
            && remove_any(&old).is_err()
        {
            left.push(old);
        }
    }
    Ok(left)
}

fn swap_dir(
    from: &Path,
    to: &Path,
    whole_dirs: bool,
    done: &mut Vec<(PathBuf, Option<PathBuf>)>,
) -> Result<(), String> {
    std::fs::create_dir_all(to).map_err(|e| format!("couldn't make {}: {e}", to.display()))?;
    let rd = std::fs::read_dir(from).map_err(|e| e.to_string())?;
    let mut entries: Vec<_> = rd.filter_map(|e| e.ok()).collect();
    entries.sort_by_key(|e| e.file_name());
    for e in entries {
        let name = e.file_name();
        if protected(&name.to_string_lossy()) {
            continue;
        }
        let src = e.path();
        let dst = to.join(&name);
        if src.is_dir() && !whole_dirs {
            if dst.is_file() {
                return Err(format!(
                    "{} is a file, the update has a folder there",
                    dst.display()
                ));
            }
            swap_dir(&src, &dst, false, done)?;
            continue;
        }
        let old = if dst.exists() {
            let old = old_name(&dst);
            let _ = remove_any(&old); // (an earlier update's, if it's no longer in use)
            let old = if old.exists() { unique_old(&dst) } else { old };
            std::fs::rename(&dst, &old).map_err(|e| format!("couldn't move {} aside: {e}", dst.display()))?;
            Some(old)
        } else {
            None
        };
        match std::fs::rename(&src, &dst) {
            Ok(()) => done.push((dst, old)),
            Err(e) => {
                if let Some(old) = old {
                    let _ = std::fs::rename(&old, &dst);
                }
                return Err(format!("couldn't put {} in place: {e}", dst.display()));
            }
        }
    }
    Ok(())
}

fn old_name(p: &Path) -> PathBuf {
    let mut s = p.as_os_str().to_os_string();
    s.push(".old");
    PathBuf::from(s)
}

fn unique_old(p: &Path) -> PathBuf {
    (1..)
        .map(|i| {
            let mut s = p.as_os_str().to_os_string();
            s.push(format!(".{i}.old"));
            PathBuf::from(s)
        })
        .find(|c| !c.exists())
        .expect("a free name")
}

fn remove_any(p: &Path) -> std::io::Result<()> {
    if p.is_dir() {
        std::fs::remove_dir_all(p)
    } else {
        std::fs::remove_file(p)
    }
}

/// After an update (the next start): delete the `.old` files it left and the staging folder. Ones still in use
/// stay listed for the start after.
pub fn cleanup(install: &Path) {
    let staging = install.join(STAGING);
    if !staging.is_dir() {
        return;
    }
    let list = staging.join(LEFTOVERS);
    let still: Vec<String> = std::fs::read_to_string(&list)
        .unwrap_or_default()
        .lines()
        .filter(|l| !l.trim().is_empty())
        .filter(|l| {
            let p = Path::new(l.trim());
            // (only `.old` files in the install folder, whatever the list says)
            let ours = p.starts_with(install) && p.to_string_lossy().ends_with(".old");
            ours && p.exists() && remove_any(p).is_err()
        })
        .map(str::to_string)
        .collect();
    if still.is_empty() {
        let _ = std::fs::remove_dir_all(&staging);
    } else {
        let _ = std::fs::write(&list, still.join("\n"));
    }
}

// -- where the app is installed -------------------------------------------------------------------------------------

/// Where an update goes: the folder to swap into, and whether folders are swapped whole.
#[derive(Clone, Debug, PartialEq)]
pub struct Install {
    /// Windows: the exe's folder. macOS: the folder holding `quickRS.app`.
    pub dir: PathBuf,
    pub whole_dirs: bool,
    /// macOS: the bundle (relaunched with `open`).
    pub bundle: Option<PathBuf>,
}

impl Install {
    /// From the running exe's path; None where updating isn't supported (a bare binary on macOS, other OSes).
    pub fn of_exe(exe: &Path, os: Os) -> Option<Install> {
        match os {
            Os::Windows => Some(Install {
                dir: exe.parent()?.to_path_buf(),
                whole_dirs: false,
                bundle: None,
            }),
            Os::Mac => {
                // quickRS.app/Contents/MacOS/quickrs
                let bundle = exe.ancestors().find(|a| is_app(a))?.to_path_buf();
                Some(Install {
                    dir: bundle.parent()?.to_path_buf(),
                    whole_dirs: true,
                    bundle: Some(bundle),
                })
            }
            Os::Other => None,
        }
    }

    pub fn this() -> Option<Install> {
        Install::of_exe(&std::env::current_exe().ok()?, Os::this())
    }

    pub fn staging(&self) -> PathBuf {
        self.dir.join(STAGING)
    }

    /// Whether files can be written here (a test file in the staging folder).
    pub fn writable(&self) -> bool {
        let probe = self.staging().join("probe.tmp");
        let ok = std::fs::create_dir_all(self.staging()).is_ok() && std::fs::write(&probe, b"ok").is_ok();
        let _ = std::fs::remove_file(&probe);
        ok
    }
}

/// The start after an update: tidy up.
pub fn cleanup_this() {
    if let Some(i) = Install::this() {
        cleanup(&i.dir);
    }
}

// -- fetching -------------------------------------------------------------------------------------------------------

/// Where the check asks (QUICKRS_UPDATE_URL, else the public repo).
pub fn source_url() -> String {
    std::env::var("QUICKRS_UPDATE_URL")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| LATEST_API.to_string())
}

/// An http(s) URL, or a local file (a `file://` URL or a path: tests).
fn fetch(url: &str, progress: &dyn Fn(u64, Option<u64>)) -> Result<Vec<u8>, String> {
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        let path = url.strip_prefix("file://").unwrap_or(url);
        let path = path
            .strip_prefix('/')
            .filter(|p| p.get(1..2) == Some(":"))
            .unwrap_or(path);
        return std::fs::read(path).map_err(|e| format!("couldn't read {path}: {e}"));
    }
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_connect(Some(Duration::from_secs(10)))
        .timeout_global(Some(Duration::from_secs(15 * 60)))
        .user_agent(format!("quickRS/{}", env!("CARGO_PKG_VERSION")))
        .build()
        .into();
    let resp = agent
        .get(url)
        .header(
            "Accept",
            "application/vnd.github+json, application/octet-stream;q=0.9, */*;q=0.8",
        )
        .call()
        .map_err(|e| match e {
            ureq::Error::StatusCode(404) => "no release found".to_string(),
            ureq::Error::StatusCode(403) => {
                "GitHub said not now (too many checks); try again later".to_string()
            }
            e => format!("couldn't reach GitHub: {e}"),
        })?;
    let total = resp.body().content_length();
    let mut reader = resp.into_body().into_with_config().limit(MAX_DOWNLOAD).reader();
    let mut out = Vec::with_capacity(total.unwrap_or(0).min(MAX_DOWNLOAD) as usize);
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = reader
            .read(&mut buf)
            .map_err(|e| format!("the download broke off: {e}"))?;
        if n == 0 {
            break;
        }
        out.extend_from_slice(&buf[..n]);
        progress(out.len() as u64, total);
    }
    Ok(out)
}

/// Ask for the latest release.
pub fn check_now(url: &str) -> Result<Release, String> {
    let bytes = fetch(url, &|_, _| {})?;
    parse_release(&String::from_utf8_lossy(&bytes))
}

/// Download, verify, unpack and swap in `rel` for this OS. Ok(Installed) means it's in place and the app should
/// start again; Ok(Browser) that this install can't update itself and the release page was opened instead.
pub fn install(rel: &Release, progress: &dyn Fn(u64, Option<u64>)) -> Result<Installed, String> {
    let os = Os::this();
    let Some(inst) = Install::this().filter(|i| i.writable()) else {
        open_url(&rel.page);
        return Ok(Installed::Browser);
    };
    install_into(rel, os, &inst, progress)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Installed {
    Done,
    Browser,
}

/// `install`'s work, on a given install folder (tests run it on a temporary one).
pub fn install_into(
    rel: &Release,
    os: Os,
    inst: &Install,
    progress: &dyn Fn(u64, Option<u64>),
) -> Result<Installed, String> {
    let asset = pick_asset(rel, os).ok_or_else(|| {
        format!(
            "the release has no {}",
            asset_name(&rel.version, os).unwrap_or_else(|| "build for this system".into())
        )
    })?;
    let sums_asset = rel
        .assets
        .iter()
        .find(|a| a.name.eq_ignore_ascii_case(SUMS_NAME))
        .ok_or_else(|| format!("the release has no {SUMS_NAME}, so its download can't be checked"))?;
    let sums = parse_sums(&String::from_utf8_lossy(&fetch(&sums_asset.url, &|_, _| {})?));
    let zip = fetch(&asset.url, progress)?;
    verify(&asset.name, &zip, &sums)?;
    let staging = inst.staging();
    let unpacked = staging.join(rel.version.to_string());
    extract_zip(&zip, &unpacked)?;
    let root = staged_root(&unpacked, os).ok_or("the update's zip is empty")?;
    let left = swap_in(&root, &inst.dir, inst.whole_dirs)?;
    let list: Vec<String> = left.iter().map(|p| p.display().to_string()).collect();
    let _ = std::fs::write(staging.join(LEFTOVERS), list.join("\n"));
    let _ = std::fs::remove_dir_all(&unpacked);
    Ok(Installed::Done)
}

/// Start the (new) app; it waits for this one to close (`--after-update <pid>`).
pub fn relaunch() -> std::io::Result<()> {
    let pid = std::process::id().to_string();
    let inst = Install::this();
    if let Some(bundle) = inst.as_ref().and_then(|i| i.bundle.as_ref()) {
        std::process::Command::new("open")
            .arg("-n")
            .arg(bundle)
            .args(["--args", "--after-update", &pid])
            .spawn()?;
        return Ok(());
    }
    // (the path it was started from, where the new exe now is; never the renamed old one)
    let exe = std::env::current_exe()?;
    let exe = match exe.to_str().and_then(|s| s.strip_suffix(".old")) {
        Some(s) => PathBuf::from(s),
        None => exe,
    };
    std::process::Command::new(exe)
        .args(["--after-update", &pid])
        .spawn()?;
    Ok(())
}

/// The new app's first step after an update: wait (at most 15 s) for the old one to have closed, so the sound
/// devices and files are free.
pub fn wait_for_exit(pid: u32) {
    #[cfg(windows)]
    unsafe {
        use windows::Win32::Foundation::CloseHandle;
        use windows::Win32::System::Threading::{OpenProcess, PROCESS_SYNCHRONIZE, WaitForSingleObject};
        if let Ok(h) = OpenProcess(PROCESS_SYNCHRONIZE, false, pid) {
            WaitForSingleObject(h, 15_000);
            let _ = CloseHandle(h);
        }
    }
    #[cfg(not(windows))]
    {
        let alive = || {
            std::process::Command::new("kill")
                .args(["-0", &pid.to_string()])
                .stderr(std::process::Stdio::null())
                .status()
                .is_ok_and(|s| s.success())
        };
        let t0 = std::time::Instant::now();
        while alive() && t0.elapsed() < Duration::from_secs(15) {
            std::thread::sleep(Duration::from_millis(100));
        }
    }
}

/// A new issue on the public repo (Settings > Updates > Report a bug; owner request, 2026-10-29).
pub const ISSUES_NEW: &str = "https://github.com/arpan2711/quickRS-player/issues/new";

/// The bug report's page: a new issue with the version and OS filled in and a short form to fill (nothing else from
/// this computer: no paths, no device names).
pub fn bug_report_url() -> String {
    let body = format!(
        "**quickRS {}** on {} ({})

**What happened**


**What you did just before (song, part, keys pressed)**


         **What you expected**


**Your audio setup** (interface or cable, Settings > Audio input / output)


         If quickRS crashed or showed an error, please attach quickrs.log (Settings > Updates > Show the log file).",
        Version::current(),
        std::env::consts::OS,
        std::env::consts::ARCH
    );
    format!(
        "{ISSUES_NEW}?title={}&body={}",
        url_encode("Bug: "),
        url_encode(&body)
    )
}

/// Percent-encoding for a URL's query (everything but unreserved characters).
fn url_encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// Open a web page in the default browser.
pub fn open_url(url: &str) {
    #[cfg(windows)]
    let r = std::process::Command::new("rundll32")
        .args(["url.dll,FileProtocolHandler", url])
        .spawn();
    #[cfg(target_os = "macos")]
    let r = std::process::Command::new("open").arg(url).spawn();
    #[cfg(not(any(windows, target_os = "macos")))]
    let r = std::process::Command::new("xdg-open").arg(url).spawn();
    if let Err(e) = r {
        crate::applog::write(&format!("update: couldn't open {url}: {e}"));
    }
}

// -- the background worker the UI polls -------------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
pub enum Phase {
    Idle,
    Checking,
    /// The latest release is this version or older.
    UpToDate(Version),
    Available(Release),
    /// Downloading (bytes so far, of).
    Downloading(u64, Option<u64>),
    /// Swapped in: the app starts again.
    Restart,
    /// This install can't update itself: the release page was opened.
    Browser,
    Failed(String),
}

impl Phase {
    pub fn busy(&self) -> bool {
        matches!(self, Phase::Checking | Phase::Downloading(..) | Phase::Restart)
    }
}

pub struct Updater {
    phase: Arc<Mutex<Phase>>,
    ctx: Option<eframe::egui::Context>,
    /// "Later": the notice is gone until the next start (or a check from Settings).
    pub later: bool,
    /// The last check was asked for from Settings (its answer shows there, not as a notice).
    pub manual: bool,
    /// The release being installed (Update now).
    pub installing: Option<Release>,
}

impl Updater {
    pub fn new(ctx: Option<eframe::egui::Context>) -> Updater {
        Updater {
            phase: Arc::new(Mutex::new(Phase::Idle)),
            ctx,
            later: false,
            manual: false,
            installing: None,
        }
    }

    pub fn phase(&self) -> Phase {
        self.phase.lock().map(|p| p.clone()).unwrap_or(Phase::Idle)
    }

    fn set(phase: &Arc<Mutex<Phase>>, ctx: &Option<eframe::egui::Context>, p: Phase) {
        if let Ok(mut g) = phase.lock() {
            *g = p;
        }
        if let Some(c) = ctx {
            c.request_repaint();
        }
    }

    /// The check on start: when it's on and the last one was a day ago or more.
    pub fn auto_check(&mut self, config: &mut Config) {
        if !config.bool(KEY_CHECK, true) {
            return;
        }
        let now = now_s();
        let last = config.f64(KEY_LAST, 0.0) as u64;
        if now.saturating_sub(last) < CHECK_EVERY_S {
            return;
        }
        config.set(KEY_LAST, now);
        let _ = config.save();
        self.check(false);
    }

    /// Ask GitHub (in the background).
    pub fn check(&mut self, manual: bool) {
        if self.phase().busy() {
            return;
        }
        self.manual = manual;
        self.installing = None;
        if manual {
            self.later = false;
        }
        let (phase, ctx) = (self.phase.clone(), self.ctx.clone());
        Self::set(&phase, &ctx, Phase::Checking);
        let url = source_url();
        let _ = std::thread::Builder::new()
            .name("update-check".into())
            .spawn(move || {
                let p = match check_now(&url) {
                    Ok(rel) if rel.version > Version::current() => Phase::Available(rel),
                    Ok(rel) => Phase::UpToDate(rel.version),
                    Err(e) => Phase::Failed(e),
                };
                if let Phase::Failed(e) = &p {
                    crate::applog::write(&format!("update check: {e}"));
                }
                Self::set(&phase, &ctx, p);
            });
    }

    /// Update now.
    pub fn install(&mut self, rel: Release) {
        if self.phase().busy() {
            return;
        }
        self.installing = Some(rel.clone());
        let (phase, ctx) = (self.phase.clone(), self.ctx.clone());
        Self::set(&phase, &ctx, Phase::Downloading(0, None));
        let _ = std::thread::Builder::new()
            .name("update-install".into())
            .spawn(move || {
                let last = Mutex::new(0u64);
                let progress = |n: u64, total: Option<u64>| {
                    // (a repaint every quarter of a megabyte is plenty)
                    if let Ok(mut l) = last.lock()
                        && n - *l >= 256 * 1024
                    {
                        *l = n;
                        Self::set(&phase, &ctx, Phase::Downloading(n, total));
                    }
                };
                let p = match install(&rel, &progress) {
                    Ok(Installed::Done) => Phase::Restart,
                    Ok(Installed::Browser) => Phase::Browser,
                    Err(e) => Phase::Failed(e),
                };
                crate::applog::write(&format!("update to {}: {p:?}", rel.version));
                Self::set(&phase, &ctx, p);
            });
    }

    /// The app is to start again (taken once).
    pub fn take_restart(&mut self) -> bool {
        if self.phase() == Phase::Restart {
            Self::set(&self.phase, &None, Phase::Idle);
            return true;
        }
        false
    }

    /// The release to offer in the notice: newer, not skipped, not put off.
    pub fn offer(&self, config: &Config) -> Option<Release> {
        if self.later || self.manual {
            return None;
        }
        match self.phase() {
            Phase::Available(rel) if config.str(KEY_SKIP, "") != rel.version.to_string() => Some(rel),
            _ => None,
        }
    }
}

fn now_s() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

// -- what the user sees -----------------------------------------------------------------------------------------------

/// Settings > Updates and the notice: `update:<what>` (check, install, notes, page, skip, unskip, later).
pub fn act(st: &mut crate::state::AppState, what: &str) {
    match what {
        "check" => st.updater.check(true),
        "install" => {
            let rel = match st.updater.phase() {
                Phase::Available(rel) => Some(rel),
                _ => st.updater.installing.clone(),
            };
            if let Some(rel) = rel {
                st.updater.install(rel);
            }
        }
        "notes" => {
            let page = match st.updater.phase() {
                Phase::Available(rel) => rel.page,
                _ => st
                    .updater
                    .installing
                    .as_ref()
                    .map(|r| r.page.clone())
                    .unwrap_or_else(|| RELEASES_PAGE.into()),
            };
            open_url(&page);
        }
        "page" => open_url(RELEASES_PAGE),
        "bug" => open_url(&bug_report_url()),
        "log" => match crate::applog::path() {
            Some(p) if p.exists() => crate::recording::reveal(&p),
            Some(p) => {
                if let Some(dir) = p.parent() {
                    crate::recording::open_dir(dir);
                }
            }
            None => {}
        },
        "skip" => {
            if let Phase::Available(rel) = st.updater.phase() {
                st.config.set(KEY_SKIP, rel.version.to_string());
                st.save_config();
            }
            st.updater.later = true;
        }
        "unskip" => {
            st.config.remove(KEY_SKIP);
            st.save_config();
            st.updater.later = false;
        }
        "later" => {
            st.updater.later = true;
            st.updater.installing = None;
        }
        _ => {}
    }
}

/// The phase in words (Settings, and the notice while installing).
pub fn describe(phase: &Phase) -> String {
    match phase {
        Phase::Idle => String::new(),
        Phase::Checking => "Checking for a newer version\u{2026}".into(),
        Phase::UpToDate(latest) => format!("quickRS is up to date (the latest release is {latest})"),
        Phase::Available(rel) => format!("quickRS {} is out", rel.version),
        Phase::Downloading(n, total) => match total {
            Some(t) if *t > 0 => format!("Downloading\u{2026} {:.1} of {:.1} MB", mb(*n), mb(*t)),
            _ => format!("Downloading\u{2026} {:.1} MB", mb(*n)),
        },
        Phase::Restart => "Installed: quickRS is starting again\u{2026}".into(),
        Phase::Browser => {
            "This folder can't be written to, so the release page is open in your browser: download it from there"
                .into()
        }
        Phase::Failed(e) => format!("Couldn't update: {e}"),
    }
}

fn mb(n: u64) -> f64 {
    n as f64 / (1024.0 * 1024.0)
}

/// The notice at the bottom left, over the page: a newer version (Update now, Later, Skip this version, its
/// notes), then how the update is going. Not over a song (it waits until the song is closed).
pub fn notice(ui: &eframe::egui::Ui, st: &mut crate::state::AppState, area: eframe::egui::Rect) {
    // the desktop zips can't update an Android install: the APK is built and installed separately
    if cfg!(target_os = "android") {
        return;
    }
    use eframe::egui::{Align2, CursorIcon, Rect, Sense, pos2, vec2};
    use qrs_ui::paint;
    use qrs_ui::text::{self, Style};
    use qrs_ui::theme::t;
    use qrs_ui::widgets::{self, ButtonStyle, interact};

    let phase = st.updater.phase();
    let installing = st
        .updater
        .installing
        .clone()
        .filter(|_| !st.updater.manual && !st.updater.later);
    let offer = st.updater.offer(&st.config);
    let title = match (&offer, &installing) {
        (Some(rel), _) => format!("quickRS {} is out", rel.version),
        (None, Some(rel)) => format!("Updating to quickRS {}", rel.version),
        _ => return,
    };
    let th = t();
    let p = ui.painter().clone();
    let ctx = ui.ctx().clone();
    let (pad, w) = (22.0, (420.0f32).min(area.width() - 40.0));
    let h = pad + 16.0 + 26.0 + 20.0 + 16.0 + 44.0 + pad;
    let card = Rect::from_min_size(pos2(area.min.x + 28.0, area.max.y - 110.0 - h), vec2(w, h));
    // (clicks on the card don't reach the page under it)
    let _ = interact(ui, card, "update-card", Sense::click());
    paint::floating(&p, card, Some(22.0));
    let x = card.min.x + pad;
    text::caps(
        &p,
        if offer.is_some() {
            "UPDATE AVAILABLE"
        } else {
            "UPDATE"
        },
        pos2(x, card.min.y + pad),
        11.0,
        th.caption,
        Align2::LEFT_TOP,
        None,
    );
    text::draw(
        &p,
        &title,
        pos2(x, card.min.y + pad + 16.0),
        Align2::LEFT_TOP,
        Style::new(17.0, 800, th.text),
        Some(card.width() - 2.0 * pad),
    );
    let line_y = card.min.y + pad + 16.0 + 26.0;
    let mut acts: Vec<&str> = Vec::new();
    if offer.is_some() {
        let r = text::draw(
            &p,
            &format!("You have {}.", Version::current()),
            pos2(x, line_y),
            Align2::LEFT_TOP,
            Style::new(12.0, 500, th.caption),
            None,
        );
        let link_st = Style::new(12.0, 700, th.accent_ink);
        let lr = text::draw(
            &p,
            "Release notes",
            pos2(r.max.x + 8.0, line_y),
            Align2::LEFT_TOP,
            link_st,
            None,
        );
        let resp = interact(ui, lr.expand(3.0), "update-notes", Sense::click());
        if resp.hovered() {
            ctx.set_cursor_icon(CursorIcon::PointingHand);
            p.line_segment(
                [pos2(lr.min.x, lr.max.y), pos2(lr.max.x, lr.max.y)],
                (1.0, th.accent_ink),
            );
        }
        if resp.clicked() {
            acts.push("notes");
        }
    } else {
        let failed = matches!(phase, Phase::Failed(_));
        text::draw(
            &p,
            &describe(&phase),
            pos2(x, line_y),
            Align2::LEFT_TOP,
            Style::new(12.0, 500, if failed { th.text } else { th.caption }),
            Some(card.width() - 2.0 * pad),
        );
        if phase.busy() {
            ctx.request_repaint_after(Duration::from_millis(250));
        }
    }
    let by = card.max.y - pad - 44.0;
    let style = |primary: bool| ButtonStyle {
        size: 14.0,
        r: Some(14.0),
        primary,
        ..Default::default()
    };
    // the buttons from the right
    let buttons: Vec<(&str, &str, bool)> = if offer.is_some() {
        vec![
            ("Update now", "install", true),
            ("Later", "later", false),
            ("Skip this version", "skip", false),
        ]
    } else {
        match phase {
            Phase::Failed(_) => vec![("Close", "later", false), ("Release page", "notes", false)],
            Phase::Browser => vec![("Close", "later", false)],
            _ => Vec::new(),
        }
    };
    let mut right = card.max.x - pad;
    for (label, what, primary) in buttons {
        let bw = widgets::button_width(&ctx, label, "", 14.0, false);
        let r = Rect::from_min_size(pos2(right - bw, by), vec2(bw, 44.0));
        if widgets::button(ui, r, ("update-btn", what), label, "", style(primary)).clicked() {
            acts.push(what);
        }
        right = r.min.x - 10.0;
    }
    for a in acts {
        act(st, a);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A bug report opens a new issue on the public repo with the version filled in, the query encoded.
    #[test]
    fn a_bug_report_is_a_filled_in_issue() {
        let u = bug_report_url();
        assert!(u.starts_with(ISSUES_NEW));
        assert!(u.contains(&url_encode(&Version::current().to_string())));
        assert!(!u.contains(' ') && !u.contains('\n'), "{u}");
        assert_eq!(url_encode("a b/c\n"), "a%20b%2Fc%0A");
    }
    use std::io::Write;

    fn v(s: &str) -> Version {
        Version::parse(s).unwrap()
    }

    #[test]
    fn versions_compare() {
        assert!(v("0.2.0") > v("0.1.0"));
        assert!(v("v0.10.0") > v("0.9.9"));
        assert!(v("1.0.0") > v("0.99.99"));
        assert_eq!(v("v0.2"), v("0.2.0"));
        assert_eq!(v("0.2.0+build.5"), v("0.2.0"));
        assert!(v("0.2.0-beta.1") < v("0.2.0"));
        assert!(v("0.2.0-beta.2") > v("0.2.0-beta.1"));
        assert!(v("0.2.0-beta.10") > v("0.2.0-beta.2"));
        assert!(v("0.2.0-alpha") < v("0.2.0-beta"));
        assert!(v("0.2.0-1") < v("0.2.0-alpha"));
        assert_eq!(v("1.2.3-rc.1").to_string(), "1.2.3-rc.1");
        assert!(Version::parse("latest").is_none());
        assert!(Version::parse("1.2.3.4").is_none());
        assert_eq!(
            Version::parse(env!("CARGO_PKG_VERSION")).unwrap().to_string(),
            env!("CARGO_PKG_VERSION")
        );
    }

    fn release_json(tag: &str) -> String {
        serde_json::json!({
            "tag_name": tag,
            "html_url": "https://github.com/arpan2711/quickRS-player/releases/tag/v0.3.0",
            "draft": false,
            "prerelease": false,
            "assets": [
                {"name": "quickRS-0.3.0-windows-x64.zip", "browser_download_url": "https://x/w.zip"},
                {"name": "quickRS-0.3.0-macos-universal.zip", "browser_download_url": "https://x/m.zip"},
                {"name": "SHA256SUMS.txt", "browser_download_url": "https://x/sums"},
            ]
        })
        .to_string()
    }

    #[test]
    fn releases_parse_and_assets_are_picked_per_os() {
        let rel = parse_release(&release_json("v0.3.0")).unwrap();
        assert_eq!(rel.version, v("0.3.0"));
        assert!(rel.page.ends_with("/v0.3.0"));
        assert_eq!(pick_asset(&rel, Os::Windows).unwrap().url, "https://x/w.zip");
        assert_eq!(pick_asset(&rel, Os::Mac).unwrap().url, "https://x/m.zip");
        assert!(pick_asset(&rel, Os::Other).is_none());
        // a tag that isn't this version's assets: nothing to pick
        let other = parse_release(&release_json("v0.4.0")).unwrap();
        assert!(pick_asset(&other, Os::Windows).is_none());
        let mut pre: serde_json::Value = serde_json::from_str(&release_json("v0.3.0")).unwrap();
        pre["prerelease"] = true.into();
        assert!(parse_release(&pre.to_string()).is_err());
        assert!(parse_release("{}").is_err());
    }

    #[test]
    fn sums_parse_and_mismatches_are_refused() {
        let zip = b"pretend zip".to_vec();
        let text = format!(
            "# quickRS 0.3.0\n{}\n{} *quickRS-0.3.0-macos-universal.zip\n\nnot a line\n",
            sums_line("quickRS-0.3.0-windows-x64.zip", &zip),
            "AB".repeat(32)
        );
        let sums = parse_sums(&text);
        assert_eq!(sums.len(), 2);
        assert_eq!(sums["quickRS-0.3.0-macos-universal.zip"], "ab".repeat(32));
        assert!(verify("quickRS-0.3.0-windows-x64.zip", &zip, &sums).is_ok());
        assert!(verify("quickRS-0.3.0-windows-x64.zip", b"tampered", &sums).is_err());
        assert!(verify("quickRS-0.3.0-macos-universal.zip", &zip, &sums).is_err());
        assert!(verify("missing.zip", &zip, &sums).is_err());
        // (sha256 of "abc", the standard test vector)
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("qrs-update-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn zip_of(files: &[(&str, &[u8])]) -> Vec<u8> {
        let mut buf = std::io::Cursor::new(Vec::new());
        {
            let mut z = zip::ZipWriter::new(&mut buf);
            let opts = zip::write::SimpleFileOptions::default();
            for (name, data) in files {
                z.start_file(*name, opts).unwrap();
                z.write_all(data).unwrap();
            }
            z.finish().unwrap();
        }
        buf.into_inner()
    }

    #[test]
    fn the_swap_replaces_programs_and_leaves_user_files() {
        let d = tmp("swap");
        let install = d.join("quickRS");
        std::fs::create_dir_all(install.join("licences")).unwrap();
        std::fs::create_dir_all(install.join(".quickrs-cache")).unwrap();
        for (f, t) in [
            ("quickrs.exe", "old exe"),
            ("README.txt", "old readme"),
            ("licences/quickRS.txt", "old licence"),
            ("keys.json", "my keys"),
            ("config.json", "my config"),
            ("userdata.json", "my scores"),
            ("mine.txt", "not in the zip"),
            (".quickrs-cache/library.json", "index"),
        ] {
            std::fs::write(install.join(f), t).unwrap();
        }
        // the zip: one folder, with programs and (wrongly) a keys.json and config.json
        let zip = zip_of(&[
            ("quickRS-0.3.0/quickrs.exe", b"new exe"),
            ("quickRS-0.3.0/README.txt", b"new readme"),
            ("quickRS-0.3.0/licences/quickRS.txt", b"new licence"),
            ("quickRS-0.3.0/licences/extra.txt", b"new file"),
            ("quickRS-0.3.0/keys.json", b"zip keys"),
            ("quickRS-0.3.0/config.json", b"zip config"),
            ("quickRS-0.3.0/.quickrs-cache/library.json", b"zip index"),
        ]);
        let unpacked = install.join(".update/0.3.0");
        extract_zip(&zip, &unpacked).unwrap();
        let root = staged_root(&unpacked, Os::Windows).unwrap();
        assert!(root.ends_with("quickRS-0.3.0"));
        // Windows: quickrs.exe really running (a copy of ping, held for a few seconds): it can be renamed, not
        // overwritten or deleted
        #[cfg(windows)]
        let mut running = {
            let sys = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
            std::fs::copy(
                Path::new(&sys).join(r"System32\PING.EXE"),
                install.join("quickrs.exe"),
            )
            .unwrap();
            std::process::Command::new(install.join("quickrs.exe"))
                .args(["-n", "30", "127.0.0.1"])
                .stdout(std::process::Stdio::null())
                .spawn()
                .unwrap()
        };
        let left = swap_in(&root, &install, false).unwrap();
        let read = |f: &str| std::fs::read_to_string(install.join(f)).unwrap();
        assert_eq!(read("quickrs.exe"), "new exe");
        assert_eq!(read("README.txt"), "new readme");
        assert_eq!(read("licences/quickRS.txt"), "new licence");
        assert_eq!(read("licences/extra.txt"), "new file");
        assert_eq!(read("keys.json"), "my keys");
        assert_eq!(read("config.json"), "my config");
        assert_eq!(read("userdata.json"), "my scores");
        assert_eq!(read("mine.txt"), "not in the zip");
        assert_eq!(read(".quickrs-cache/library.json"), "index");
        // the running exe's .old couldn't be deleted: it's left for the next start
        #[cfg(windows)]
        assert_eq!(left, vec![install.join("quickrs.exe.old")]);
        assert!(!install.join("README.txt.old").exists());
        std::fs::write(
            install.join(".update").join(LEFTOVERS),
            left.iter()
                .map(|p| p.display().to_string())
                .collect::<Vec<_>>()
                .join("\n"),
        )
        .unwrap();
        // a listed path outside the install folder is never deleted
        let outside = d.join("outside.old");
        std::fs::write(&outside, "x").unwrap();
        let mut list = std::fs::read_to_string(install.join(".update").join(LEFTOVERS)).unwrap();
        list.push_str(&format!(
            "
{}",
            outside.display()
        ));
        std::fs::write(install.join(".update").join(LEFTOVERS), list).unwrap();
        #[cfg(windows)]
        {
            // still running: the next start leaves it listed
            cleanup(&install);
            assert!(install.join("quickrs.exe.old").exists());
            let _ = running.kill();
            let _ = running.wait();
        }
        cleanup(&install);
        assert!(outside.exists());
        assert!(!install.join("quickrs.exe.old").exists());
        assert!(!install.join(".update").exists());
        assert_eq!(read("quickrs.exe"), "new exe");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_failed_swap_puts_everything_back() {
        let d = tmp("rollback");
        let install = d.join("app");
        std::fs::create_dir_all(&install).unwrap();
        std::fs::write(install.join("a.exe"), "old a").unwrap();
        // "b" is a file in the install and a folder in the update: the swap stops there
        std::fs::write(install.join("b"), "old b").unwrap();
        let staged = d.join("staged");
        std::fs::create_dir_all(staged.join("b")).unwrap();
        std::fs::write(staged.join("a.exe"), "new a").unwrap();
        std::fs::write(staged.join("b/inner.txt"), "x").unwrap();
        assert!(swap_in(&staged, &install, false).is_err());
        assert_eq!(std::fs::read_to_string(install.join("a.exe")).unwrap(), "old a");
        assert_eq!(std::fs::read_to_string(install.join("b")).unwrap(), "old b");
        assert!(!install.join("a.exe.old").exists());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn the_mac_bundle_is_swapped_whole() {
        let d = tmp("mac");
        let apps = d.join("Applications");
        let bundle = apps.join("quickRS.app");
        std::fs::create_dir_all(bundle.join("Contents/MacOS")).unwrap();
        std::fs::write(bundle.join("Contents/MacOS/quickrs"), "old").unwrap();
        std::fs::write(bundle.join("Contents/old-only.txt"), "gone after").unwrap();
        let inst = Install::of_exe(&bundle.join("Contents/MacOS/quickrs"), Os::Mac).unwrap();
        assert_eq!(inst.dir, apps);
        assert!(inst.whole_dirs);
        let zip = zip_of(&[("quickRS.app/Contents/MacOS/quickrs", b"new")]);
        let unpacked = inst.staging().join("0.3.0");
        extract_zip(&zip, &unpacked).unwrap();
        let root = staged_root(&unpacked, Os::Mac).unwrap();
        assert_eq!(root, unpacked);
        swap_in(&root, &inst.dir, true).unwrap();
        assert_eq!(
            std::fs::read_to_string(bundle.join("Contents/MacOS/quickrs")).unwrap(),
            "new"
        );
        assert!(!bundle.join("Contents/old-only.txt").exists());
        assert!(Install::of_exe(Path::new("/usr/local/bin/quickrs"), Os::Mac).is_none());
        let _ = std::fs::remove_dir_all(&d);
    }

    /// The real GitHub API (network: `cargo test -p qrs-app live_check -- --ignored`).
    #[test]
    #[ignore]
    fn live_check() {
        let rel = check_now(&source_url()).unwrap();
        println!(
            "latest: {} at {} ({} assets)",
            rel.version,
            rel.page,
            rel.assets.len()
        );
        for a in &rel.assets {
            println!("  {} {}", a.name, a.url);
        }
    }

    #[test]
    fn unsafe_zip_paths_are_refused() {
        let d = tmp("slip");
        let zip = zip_of(&[("../evil.txt", b"x")]);
        assert!(extract_zip(&zip, &d.join("out")).is_err());
        assert!(!d.join("evil.txt").exists());
        let _ = std::fs::remove_dir_all(&d);
    }

    /// The whole install from local files (QUICKRS_UPDATE_URL can point at such a release JSON too).
    #[test]
    fn install_from_a_local_release_checks_the_sum() {
        let d = tmp("local");
        let install = d.join("quickRS");
        std::fs::create_dir_all(&install).unwrap();
        std::fs::write(install.join("quickrs.exe"), "old").unwrap();
        let zip = zip_of(&[("quickRS-0.3.0/quickrs.exe", b"new")]);
        let zip_path = d.join("w.zip");
        std::fs::write(&zip_path, &zip).unwrap();
        let sums_path = d.join("sums.txt");
        let rel = |zip_url: &Path| Release {
            version: v("0.3.0"),
            page: RELEASES_PAGE.into(),
            assets: vec![
                Asset {
                    name: "quickRS-0.3.0-windows-x64.zip".into(),
                    url: zip_url.display().to_string(),
                },
                Asset {
                    name: SUMS_NAME.into(),
                    url: format!("file://{}", sums_path.display()),
                },
            ],
        };
        let inst = Install {
            dir: install.clone(),
            whole_dirs: false,
            bundle: None,
        };
        // a wrong sum: refused, nothing changed
        std::fs::write(
            &sums_path,
            format!("{}  quickRS-0.3.0-windows-x64.zip\n", "0".repeat(64)),
        )
        .unwrap();
        assert!(install_into(&rel(&zip_path), Os::Windows, &inst, &|_, _| {}).is_err());
        assert_eq!(
            std::fs::read_to_string(install.join("quickrs.exe")).unwrap(),
            "old"
        );
        // the right one
        std::fs::write(&sums_path, sums_line("quickRS-0.3.0-windows-x64.zip", &zip)).unwrap();
        assert_eq!(
            install_into(&rel(&zip_path), Os::Windows, &inst, &|_, _| {}),
            Ok(Installed::Done)
        );
        assert_eq!(
            std::fs::read_to_string(install.join("quickrs.exe")).unwrap(),
            "new"
        );
        assert!(!install.join(".update/0.3.0").exists());
        let _ = std::fs::remove_dir_all(&d);
    }
}
