//! Picking a capture on TONE3000 (new in quickRS, P31.3, D80), on its free tier for open-source apps: the "select"
//! OAuth flow (https://www.tone3000.com/api, github.com/tone-3000/api). quickRS opens TONE3000's own page in the
//! browser (`prompt=select_tone`); you sign in there (once: the site remembers you) and pick a tone; the browser
//! comes back to `http://127.0.0.1:<port>/callback` (a loopback redirect, accepted without registering it) with a
//! code and the tone's id; the code becomes a token (PKCE, no secret in the app); the tone's models are listed and
//! the first that runs here, standard size first, is downloaded into the library with its creator and licence, then
//! its level is measured.
//!
//! Everything runs on the pick's own thread; the UI reads where it is (`Pick::phase`) and the bytes of the file coming
//! in (`Pick::progress`, atomics) every frame (P38.3, D157: `crate::downloads` keeps the picks and draws them).
//!
//! Rules kept (the free tier's): downloads only when you pick a tone; nothing cached or mirrored but the files you
//! picked; "Powered by TONE3000", the creator and the licence shown (`ui`). The client id is the owner's TONE3000
//! publishable key (`t3k_pub_...`, Settings > API Keys on tone3000.com), config.json `tone3000_client_id` (or
//! QUICKRS_T3K_CLIENT_ID); none is built in yet. The token lives in memory for the one pick and is never written
//! or logged.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::Value;
use sha2::{Digest, Sha256};

use super::suggest::Unit;
use super::{Item, Kind};

pub const SITE: &str = "https://www.tone3000.com";
const API: &str = "https://www.tone3000.com/api/v1";
/// How long the browser has to come back.
pub const WAIT: Duration = Duration::from_secs(10 * 60);
/// A model file is never bigger than this.
const MAX_FILE: u64 = 64 * 1024 * 1024;
/// Every architecture nam-rs runs, asked for one at a time on the models list (P37.2 asked for `1_2_custom` at once,
/// which TONE3000 refuses: "architecture must be '1', '2', or 'custom' when provided", the owner's pick of
/// 2026-11-05; D158). The picker itself is opened without one: it then shows A1 and custom tones.
const ARCHITECTURES: [&str; 3] = ["1", "2", "custom"];
/// The sizes tried, in order (the standard network sounds best; the smaller ones if it's missing).
const SIZES: [&str; 5] = ["standard", "lite", "feather", "nano", "custom"];

/// The publishable key, if there is one.
pub fn client_id(config: &qrs_library::Config) -> Option<String> {
    std::env::var("QUICKRS_T3K_CLIENT_ID")
        .ok()
        .or_else(|| {
            config
                .get("tone3000_client_id")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .or_else(|| super::with(|l| l.key()))
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Why a pick didn't end in a capture.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Why {
    /// TONE3000 couldn't be reached, or a download broke off.
    Network,
    /// TONE3000 answered, but with an error (sign-in refused, a file refused).
    Site,
    /// The browser didn't come back in time.
    TimedOut,
    /// The files came, but none of them runs here.
    Invalid,
    /// Anything else (the loopback couldn't listen).
    Other,
}

/// Where a pick is.
#[derive(Clone, Debug, PartialEq)]
pub enum Phase {
    /// The page is open in the browser: pick a tone there.
    InBrowser,
    /// Picked: the code becoming a token.
    SigningIn,
    /// Reading the tone and the list of its files.
    Reading,
    /// File `file` of `files` (from 1) coming in; its bytes in `Pick::progress`.
    Downloading {
        file: usize,
        files: usize,
        name: String,
    },
    /// In the library; its level being measured against the model it stands in for.
    Measuring(Box<Item>),
    Done(Box<Item>),
    Failed(Why, String),
    /// Nothing picked: cancelled here, or on TONE3000's page (`in_browser`).
    Cancelled {
        in_browser: bool,
    },
}

impl Phase {
    pub fn finished(&self) -> bool {
        matches!(self, Phase::Done(_) | Phase::Failed(..) | Phase::Cancelled { .. })
    }

    /// The capture, once it's in the library (measuring or done).
    pub fn item(&self) -> Option<&Item> {
        match self {
            Phase::Measuring(it) | Phase::Done(it) => Some(it),
            _ => None,
        }
    }
}

/// A file's bytes so far, and its size when the server said.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Progress {
    pub got: u64,
    pub total: Option<u64>,
}

impl Progress {
    /// How far, 0-1 (None: the size isn't known, an indeterminate bar).
    pub fn fraction(&self) -> Option<f32> {
        match self.total {
            Some(t) if t > 0 => Some((self.got as f64 / t as f64).clamp(0.0, 1.0) as f32),
            _ => None,
        }
    }

    /// "1.2 of 3.4 MB", or "640 KB" when the size isn't known.
    pub fn words(&self) -> String {
        match self.total {
            Some(t) if t > 0 => {
                let (g, all) = (size_words(self.got), size_words(t));
                // (the unit said once when both have it: "1.2 of 3.4 MB")
                match (g.rsplit_once(' '), all.rsplit_once(' ')) {
                    (Some((gn, gu)), Some((_, au))) if gu == au => format!("{gn} of {all}"),
                    _ => format!("{g} of {all}"),
                }
            }
            _ => size_words(self.got),
        }
    }
}

/// A size in bytes, KB or MB (decimal, as the browser says it).
pub fn size_words(b: u64) -> String {
    if b < 1000 {
        format!("{b} bytes")
    } else if b < 1_000_000 {
        format!("{} KB", (b as f64 / 1000.0).round() as u64)
    } else {
        format!("{:.1} MB", b as f64 / 1_000_000.0)
    }
}

/// (`total` when the server gave no size)
const UNKNOWN: u64 = u64::MAX;

struct Shared {
    phase: Mutex<Phase>,
    /// The tone's title, once read.
    title: Mutex<Option<String>>,
    got: AtomicU64,
    total: AtomicU64,
    cancel: AtomicBool,
    started: Instant,
    wait: Duration,
}

impl Shared {
    fn new(phase: Phase, wait: Duration) -> Arc<Shared> {
        Arc::new(Shared {
            phase: Mutex::new(phase),
            title: Mutex::new(None),
            got: AtomicU64::new(0),
            total: AtomicU64::new(UNKNOWN),
            cancel: AtomicBool::new(false),
            started: Instant::now(),
            wait,
        })
    }

    fn set(&self, ph: Phase) {
        *self.phase.lock().unwrap_or_else(|e| e.into_inner()) = ph;
    }

    fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }
}

/// A pick under way (read by the UI every frame).
pub struct Pick {
    s: Arc<Shared>,
}

impl Pick {
    pub fn phase(&self) -> Phase {
        self.s.phase.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// The tone's title, once TONE3000 has said it.
    pub fn title(&self) -> Option<String> {
        self.s.title.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// The bytes of the file coming in.
    pub fn progress(&self) -> Progress {
        let t = self.s.total.load(Ordering::Relaxed);
        Progress {
            got: self.s.got.load(Ordering::Relaxed),
            total: (t != UNKNOWN).then_some(t),
        }
    }

    pub fn cancel(&self) {
        self.s.cancel.store(true, Ordering::Relaxed);
    }

    /// How long the browser has left to come back.
    pub fn wait_left(&self) -> Duration {
        self.s.wait.saturating_sub(self.s.started.elapsed())
    }

    /// A pick shown as it would be, with no thread behind it (screenshots, tests).
    pub fn staged(phase: Phase, title: Option<&str>, progress: Progress) -> Pick {
        let s = Shared::new(phase, WAIT);
        *s.title.lock().unwrap_or_else(|e| e.into_inner()) = title.map(str::to_string);
        s.got.store(progress.got, Ordering::Relaxed);
        s.total
            .store(progress.total.unwrap_or(UNKNOWN), Ordering::Relaxed);
        Pick { s }
    }
}

/// What a pick reaches out to: TONE3000, the browser, the library, the level measurer (the real ones, or a test's).
pub(crate) struct Env {
    pub api: String,
    pub open: Box<dyn Fn(&str) + Send>,
    pub add: Box<dyn Fn(Kind, &str, &[u8], Item) -> Result<Item, String> + Send>,
    pub measure: Box<dyn Fn() + Send>,
    pub wait: Duration,
}

impl Env {
    fn real() -> Env {
        Env {
            api: API.into(),
            open: Box::new(crate::update::open_url),
            add: Box::new(|k, n, b, it| super::with(|l| l.add(k, n, b, it))),
            measure: Box::new(|| {
                let n = crate::namcheck::calibrate(false);
                if n > 0 {
                    crate::applog::write(&format!("captures: {n} level trims measured"));
                }
            }),
            wait: WAIT,
        }
    }
}

/// Open TONE3000's picker in the browser and wait for the tone on a thread. `unit`: the suggestion it was picked for
/// (the capture is then marked as that unit, D158: picked for its songs, and the suggestion shows it's in).
pub fn start(client_id: String, kind: Kind, unit: Option<&'static Unit>) -> Pick {
    start_with(client_id, kind, unit, Env::real())
}

pub(crate) fn start_with(client_id: String, kind: Kind, unit: Option<&'static Unit>, env: Env) -> Pick {
    let s = Shared::new(Phase::InBrowser, env.wait);
    let sh = s.clone();
    std::thread::spawn(move || {
        let end = match run(&client_id, kind, unit, &env, &sh) {
            Ok(it) => {
                sh.set(Phase::Measuring(Box::new(it.clone())));
                (env.measure)();
                Phase::Done(Box::new(it))
            }
            Err(End::Cancelled(in_browser)) => Phase::Cancelled { in_browser },
            Err(End::Failed(why, e)) => {
                crate::applog::write(&format!("tone3000: {e}"));
                Phase::Failed(why, e)
            }
        };
        sh.set(end);
    });
    Pick { s }
}

/// How a pick ended without a capture.
enum End {
    Failed(Why, String),
    Cancelled(bool),
}

fn fail(why: Why, e: impl Into<String>) -> End {
    End::Failed(why, e.into())
}

fn run(client_id: &str, kind: Kind, unit: Option<&'static Unit>, env: &Env, s: &Shared) -> Result<Item, End> {
    let api = env.api.as_str();
    let listener = TcpListener::bind("127.0.0.1:0")
        .map_err(|e| fail(Why::Other, format!("couldn't listen for the browser: {e}")))?;
    let port = listener
        .local_addr()
        .map_err(|e| fail(Why::Other, e.to_string()))?
        .port();
    let redirect = format!("http://127.0.0.1:{port}/callback");
    let verifier = b64url(&random(32));
    let challenge = b64url(&Sha256::digest(verifier.as_bytes()));
    let state = b64url(&random(16));
    let mut url = format!(
        "{api}/oauth/authorize?client_id={}&redirect_uri={}&response_type=code&code_challenge={challenge}\
         &code_challenge_method=S256&state={state}&prompt=select_tone&preview=true",
        enc(client_id),
        enc(&redirect),
    );
    url.push_str(match kind {
        Kind::Nam => "&format=nam&gears=amp_amp-cab",
        Kind::Pedal => "&format=nam&gears=pedal",
        Kind::Ir => "&format=ir",
    });
    (env.open)(&url);
    let q = match wait_for_callback(&listener, &s.cancel, env.wait).map_err(|e| fail(Why::Network, e))? {
        Wait::Got(q) => q,
        Wait::Cancelled => return Err(End::Cancelled(false)),
        Wait::TimedOut => {
            return Err(fail(
                Why::TimedOut,
                format!(
                    "TONE3000's page didn't come back within {} minutes",
                    env.wait.as_secs().div_ceil(60)
                ),
            ));
        }
    };
    let get = |k: &str| q.iter().find(|(a, _)| a == k).map(|(_, b)| b.clone());
    if get("state").as_deref() != Some(state.as_str()) {
        return Err(fail(
            Why::Site,
            "the sign-in came back for another request; try again",
        ));
    }
    if let Some(e) = get("error") {
        if e == "access_denied" {
            return Err(End::Cancelled(true));
        }
        return Err(fail(
            Why::Site,
            format!("TONE3000 said: {}", get("error_description").unwrap_or(e)),
        ));
    }
    if get("canceled").as_deref() == Some("true") && get("tone_id").is_none() {
        return Err(End::Cancelled(true));
    }
    let code = get("code").ok_or_else(|| fail(Why::Site, "TONE3000 sent no code back"))?;
    let tone_id: u64 = get("tone_id")
        .and_then(|t| t.parse().ok())
        .ok_or_else(|| fail(Why::Site, "no tone was picked"))?;
    if s.cancelled() {
        return Err(End::Cancelled(false));
    }
    s.set(Phase::SigningIn);
    let token = exchange(api, client_id, &code, &verifier, &redirect)?;
    s.set(Phase::Reading);
    let tone = get_json(api, &format!("{api}/tones/{tone_id}"), &token)?;
    let title = tone
        .get("title")
        .and_then(Value::as_str)
        .unwrap_or("TONE3000 tone")
        .to_string();
    *s.title.lock().unwrap_or_else(|e| e.into_inner()) = Some(title.clone());
    // every architecture, one list each (a refused one is passed over; all refused: that's the error)
    let mut list: Vec<Value> = Vec::new();
    let mut first_err = None;
    for a in ARCHITECTURES {
        if s.cancelled() {
            return Err(End::Cancelled(false));
        }
        match get_json(
            api,
            &format!("{api}/models?tone_id={tone_id}&page_size=100&architecture={a}"),
            &token,
        ) {
            Ok(v) => {
                let page = match v {
                    Value::Array(a) => a,
                    v => v
                        .get("data")
                        .and_then(Value::as_array)
                        .cloned()
                        .unwrap_or_default(),
                };
                for m in page {
                    let key = |m: &Value| m.get("id").cloned().or_else(|| m.get("model_url").cloned());
                    if !list.iter().any(|x| key(x).is_some() && key(x) == key(&m)) {
                        list.push(m);
                    }
                }
            }
            Err(e) => {
                first_err.get_or_insert(e);
            }
        }
    }
    if list.is_empty() {
        return Err(
            first_err.unwrap_or_else(|| fail(Why::Invalid, format!("\"{title}\" has no files to download")))
        );
    }
    // standard size first, then the smaller ones; the first that runs here is kept
    let rank = |m: &Value| {
        let s = m.get("size").and_then(Value::as_str).unwrap_or("custom");
        SIZES.iter().position(|x| *x == s).unwrap_or(SIZES.len())
    };
    let mut order: Vec<&Value> = list
        .iter()
        .filter(|m| m.get("model_url").and_then(Value::as_str).is_some())
        .collect();
    order.sort_by_key(|m| rank(m));
    let meta = meta_of(&tone, kind, unit);
    let mut last_err = String::from("no file to download");
    for (n, m) in order.iter().enumerate() {
        if s.cancelled() {
            return Err(End::Cancelled(false));
        }
        let file_url = m.get("model_url").and_then(Value::as_str).unwrap_or_default();
        let size = m.get("size").and_then(Value::as_str).map(str::to_string);
        let name = match (list.len() > 1, m.get("name").and_then(Value::as_str)) {
            (true, Some(n)) if !n.trim().is_empty() && !title.contains(n.trim()) => {
                format!("{title} \u{b7} {n}")
            }
            _ => title.clone(),
        };
        s.got.store(0, Ordering::Relaxed);
        s.total.store(UNKNOWN, Ordering::Relaxed);
        s.set(Phase::Downloading {
            file: n + 1,
            files: order.len(),
            name: name.clone(),
        });
        let bytes = download(file_url, &token, s)?;
        let mut it = meta.clone();
        it.size = size.filter(|s| s != "custom");
        match (env.add)(kind, &name, &bytes, it) {
            Ok(it) => return Ok(it),
            Err(e) => last_err = e,
        }
    }
    Err(fail(
        Why::Invalid,
        format!("none of \"{title}\"'s files runs here ({last_err})"),
    ))
}

fn meta_of(tone: &Value, kind: Kind, unit: Option<&Unit>) -> Item {
    let s = |v: Option<&Value>| v.and_then(Value::as_str).map(str::to_string);
    let user = tone.get("user");
    let mut it = super::blank(kind);
    it.source = "tone3000".into();
    it.creator = s(user.and_then(|u| u.get("display_name")))
        .filter(|n| !n.is_empty())
        .or_else(|| s(user.and_then(|u| u.get("username"))));
    it.licence = s(tone.get("license"));
    it.tone_id = tone.get("id").and_then(Value::as_u64);
    it.tone_url = s(tone.get("url"));
    let gear = s(tone.get("gear"));
    if kind == Kind::Nam {
        // amp-cab and full-rig captures have their cab; amps, pedals and outboard don't
        it.cab = gear.as_deref().map(|g| g == "amp-cab" || g == "full-rig");
    }
    it.gear = None;
    // (picked for a suggestion: it stands in for that unit, for the songs that use it)
    if let Some(u) = unit
        && u.kind == kind
    {
        it.rs = u.rs.iter().map(|r| r.to_string()).collect();
        it.skin = Some(u.skin.to_string());
        it.bass = u.bass;
    }
    it
}

/// What the loopback heard.
enum Wait {
    Got(Vec<(String, String)>),
    Cancelled,
    TimedOut,
}

/// The page the browser shows when it comes back (what quickRS does next is shown in quickRS).
fn callback_page(pairs: &[(String, String)]) -> String {
    let has = |k: &str| pairs.iter().any(|(a, _)| a == k);
    let (head, line) = if has("error") {
        (
            "TONE3000 sent no tone",
            "quickRS says what happened. You can close this tab.",
        )
    } else if pairs.iter().any(|(a, b)| a == "canceled" && b == "true") && !has("tone_id") {
        (
            "Nothing picked",
            "Back in quickRS, pick again whenever you like. You can close this tab.",
        )
    } else {
        (
            "Back to quickRS",
            "quickRS is downloading your pick: its progress shows in quickRS. You can close this tab.",
        )
    };
    format!(
        "<!doctype html><meta charset=utf-8><title>quickRS</title>\
         <body style=\"font:16px system-ui;background:#14202b;color:#e8e2d4;display:grid;place-items:center;height:90vh\">\
         <div><h2>{head}</h2><p>{line}</p></div>"
    )
}

/// The callback's query, or why none came (cancelled here, or the wait ran out).
fn wait_for_callback(l: &TcpListener, cancel: &AtomicBool, wait: Duration) -> Result<Wait, String> {
    l.set_nonblocking(true).map_err(|e| e.to_string())?;
    let t = Instant::now();
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Ok(Wait::Cancelled);
        }
        if t.elapsed() > wait {
            return Ok(Wait::TimedOut);
        }
        match l.accept() {
            Ok((mut s, _)) => {
                let _ = s.set_nonblocking(false);
                let _ = s.set_read_timeout(Some(Duration::from_secs(5)));
                let mut line = String::new();
                if BufReader::new(&s).read_line(&mut line).is_err() {
                    continue;
                }
                // GET /callback?code=...&state=... HTTP/1.1
                let target = line.split_whitespace().nth(1).unwrap_or("");
                if !target.starts_with("/callback") {
                    let _ = s.write_all(
                        b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                    );
                    continue;
                }
                let query = target.split_once('?').map(|(_, q)| q).unwrap_or("");
                let pairs: Vec<(String, String)> = query
                    .split('&')
                    .filter_map(|kv| kv.split_once('=').or(Some((kv, ""))))
                    .filter(|(k, _)| !k.is_empty())
                    .map(|(k, v)| (dec(k), dec(v)))
                    .collect();
                let body = callback_page(&pairs);
                let _ = write!(
                    s,
                    "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                return Ok(Wait::Got(pairs));
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(100))
            }
            Err(e) => return Err(format!("the browser's answer was lost: {e}")),
        }
    }
}

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_connect(Some(Duration::from_secs(10)))
        .timeout_recv_response(Some(Duration::from_secs(30)))
        .timeout_global(Some(Duration::from_secs(5 * 60)))
        .user_agent(format!("quickRS/{}", env!("CARGO_PKG_VERSION")))
        .http_status_as_error(false)
        .build()
        .into()
}

fn exchange(api: &str, client_id: &str, code: &str, verifier: &str, redirect: &str) -> Result<String, End> {
    let mut r = agent()
        .post(format!("{api}/oauth/token"))
        .send_form([
            ("grant_type", "authorization_code"),
            ("code", code),
            ("code_verifier", verifier),
            ("redirect_uri", redirect),
            ("client_id", client_id),
        ])
        .map_err(|e| fail(Why::Network, format!("couldn't reach TONE3000: {e}")))?;
    let status = r.status().as_u16();
    let text = r.body_mut().read_to_string().unwrap_or_default();
    let v: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
    if status != 200 {
        return Err(fail(
            Why::Site,
            format!(
                "TONE3000 didn't sign you in ({status}{})",
                v.get("error")
                    .and_then(Value::as_str)
                    .map(|e| format!(": {e}"))
                    .unwrap_or_default()
            ),
        ));
    }
    v.get("access_token")
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| fail(Why::Site, "TONE3000 sent no token"))
}

fn get_json(api: &str, url: &str, token: &str) -> Result<Value, End> {
    let mut r = agent()
        .get(url)
        .header("Authorization", format!("Bearer {token}"))
        .call()
        .map_err(|e| fail(Why::Network, format!("couldn't reach TONE3000: {e}")))?;
    let status = r.status().as_u16();
    let text = r.body_mut().read_to_string().unwrap_or_default();
    if status != 200 {
        return Err(fail(
            Why::Site,
            format!("TONE3000 answered {status} for {}", url.trim_start_matches(api)),
        ));
    }
    serde_json::from_str(&text).map_err(|e| fail(Why::Site, format!("TONE3000's answer didn't read: {e}")))
}

/// A file, its bytes counted into the pick as they come (a cancel stops it between reads).
fn download(url: &str, token: &str, s: &Shared) -> Result<Vec<u8>, End> {
    let r = agent()
        .get(url)
        .header("Authorization", format!("Bearer {token}"))
        .call()
        .map_err(|e| fail(Why::Network, format!("the download didn't start: {e}")))?;
    let status = r.status().as_u16();
    if status != 200 {
        return Err(fail(
            Why::Site,
            format!("TONE3000 refused the download ({status})"),
        ));
    }
    let total: Option<u64> = r
        .headers()
        .get("content-length")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.trim().parse().ok());
    if total.is_some_and(|t| t > MAX_FILE) {
        return Err(fail(Why::Invalid, "the file is too big to be a capture"));
    }
    s.total.store(total.unwrap_or(UNKNOWN), Ordering::Relaxed);
    let mut rd = r.into_body().into_with_config().limit(MAX_FILE).reader();
    let mut out = Vec::with_capacity(total.unwrap_or(0) as usize);
    let mut buf = vec![0u8; 32 * 1024];
    loop {
        if s.cancelled() {
            return Err(End::Cancelled(false));
        }
        match rd.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                out.extend_from_slice(&buf[..n]);
                s.got.store(out.len() as u64, Ordering::Relaxed);
            }
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(e) => return Err(fail(Why::Network, format!("the download broke off: {e}"))),
        }
    }
    if total.is_some_and(|t| (out.len() as u64) < t) {
        return Err(fail(Why::Network, "the download broke off before the end"));
    }
    Ok(out)
}

fn random(n: usize) -> Vec<u8> {
    let mut b = vec![0u8; n];
    if getrandom::fill(&mut b).is_err() {
        // (no system randomness: the clock, mixed; only the PKCE secret and the state rest on it)
        let mut x = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(7);
        for v in &mut b {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            *v = x as u8;
        }
    }
    b
}

/// Base64url without padding (RFC 4648 section 5), as PKCE wants.
pub fn b64url(b: &[u8]) -> String {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut s = String::with_capacity(b.len() * 4 / 3 + 3);
    for c in b.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        let k = c.len() + 1;
        for i in 0..k {
            s.push(A[(n >> (18 - 6 * i) & 63) as usize] as char);
        }
    }
    s
}

fn enc(s: &str) -> String {
    let mut o = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) {
            o.push(b as char);
        } else {
            o.push_str(&format!("%{b:02X}"));
        }
    }
    o
}

fn dec(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < b.len() => {
                match u8::from_str_radix(std::str::from_utf8(&b[i + 1..i + 3]).unwrap_or("zz"), 16) {
                    Ok(v) => {
                        out.push(v);
                        i += 2;
                    }
                    Err(_) => out.push(b'%'),
                }
            }
            c => out.push(c),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pkce_pieces() {
        // RFC 7636 appendix B
        let v = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
        assert_eq!(
            b64url(&Sha256::digest(v.as_bytes())),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
        assert_eq!(b64url(b"f"), "Zg");
        assert_eq!(b64url(b"fo"), "Zm8");
        assert_eq!(b64url(b"foo"), "Zm9v");
        assert_eq!(
            enc("http://127.0.0.1:5/callback"),
            "http%3A%2F%2F127.0.0.1%3A5%2Fcallback"
        );
        assert_eq!(dec("a%20b+c%2Fd"), "a b c/d");
        assert_eq!(dec("50%"), "50%");
    }

    /// The browser's side, played here: the loopback catches the callback and reads its query.
    #[test]
    fn the_callback_is_caught() {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        let t = std::thread::spawn(move || {
            let mut s = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
            s.write_all(b"GET /callback?code=abc&state=xyz&tone_id=42 HTTP/1.1\r\nHost: x\r\n\r\n")
                .unwrap();
            let mut r = String::new();
            let _ = s.read_to_string(&mut r);
            r
        });
        let Wait::Got(q) = wait_for_callback(&l, &AtomicBool::new(false), WAIT).unwrap() else {
            panic!("no callback")
        };
        assert!(q.contains(&("code".into(), "abc".into())));
        assert!(q.contains(&("tone_id".into(), "42".into())));
        let page = t.join().unwrap();
        assert!(page.starts_with("HTTP/1.1 200"));
        assert!(page.contains("progress shows in quickRS"), "{page}");
    }

    #[test]
    fn the_browser_page_says_what_came_back() {
        let p = |q: &[(&str, &str)]| {
            callback_page(
                &q.iter()
                    .map(|(a, b)| (a.to_string(), b.to_string()))
                    .collect::<Vec<_>>(),
            )
        };
        assert!(p(&[("error", "invalid_request")]).contains("sent no tone"));
        assert!(p(&[("canceled", "true")]).contains("Nothing picked"));
        assert!(p(&[("code", "c"), ("tone_id", "1")]).contains("downloading"));
    }

    #[test]
    fn a_tone_says_who_made_it() {
        let tone: Value = serde_json::from_str(
            r#"{"id": 7, "title": "Plexi", "license": "cc-by", "gear": "amp-cab", "url": "https://www.tone3000.com/tones/plexi-7",
                "user": {"username": "jo", "display_name": "Jo M"}}"#,
        )
        .unwrap();
        let it = meta_of(&tone, Kind::Nam, None);
        assert_eq!(it.creator.as_deref(), Some("Jo M"));
        assert_eq!(it.licence.as_deref(), Some("cc-by"));
        assert_eq!(it.cab, Some(true));
        assert_eq!(it.tone_id, Some(7));
        assert_eq!(it.source, "tone3000");
        assert!(it.rs.is_empty() && it.skin.is_none());
        // picked for a suggestion: it is that unit
        let u = super::super::suggest::UNITS
            .iter()
            .find(|u| u.kind == Kind::Nam)
            .unwrap();
        let it = meta_of(&tone, Kind::Nam, Some(u));
        assert_eq!(it.rs, u.rs.iter().map(|s| s.to_string()).collect::<Vec<_>>());
        assert_eq!(it.skin.as_deref(), Some(u.skin));
        // (a unit of another kind is ignored)
        let ir = super::super::suggest::UNITS
            .iter()
            .find(|u| u.kind == Kind::Ir)
            .unwrap();
        assert!(meta_of(&tone, Kind::Nam, Some(ir)).rs.is_empty());
    }

    #[test]
    fn progress_maths() {
        let p = |got, total| Progress { got, total };
        assert_eq!(p(0, Some(200)).fraction(), Some(0.0));
        assert_eq!(p(50, Some(200)).fraction(), Some(0.25));
        assert_eq!(p(300, Some(200)).fraction(), Some(1.0));
        assert_eq!(p(10, None).fraction(), None);
        assert_eq!(p(10, Some(0)).fraction(), None);
        assert_eq!(size_words(999), "999 bytes");
        assert_eq!(size_words(1500), "2 KB");
        assert_eq!(size_words(640_000), "640 KB");
        assert_eq!(size_words(1_234_567), "1.2 MB");
        assert_eq!(p(1_200_000, Some(3_400_000)).words(), "1.2 of 3.4 MB");
        assert_eq!(p(640_000, Some(3_400_000)).words(), "640 KB of 3.4 MB");
        assert_eq!(p(640_000, None).words(), "640 KB");
    }

    #[test]
    fn phases_end_where_they_should() {
        let it = Box::new(crate::captures::blank(Kind::Nam));
        assert!(!Phase::InBrowser.finished());
        assert!(!Phase::Measuring(it.clone()).finished());
        assert!(Phase::Measuring(it.clone()).item().is_some());
        assert!(Phase::Done(it).finished());
        assert!(Phase::Failed(Why::Network, "x".into()).finished());
        assert!(Phase::Cancelled { in_browser: true }.finished());
        let pk = Pick::staged(
            Phase::Reading,
            Some("Plexi"),
            Progress {
                got: 5,
                total: Some(10),
            },
        );
        assert_eq!(pk.title().as_deref(), Some("Plexi"));
        assert_eq!(pk.progress().fraction(), Some(0.5));
        assert!(pk.wait_left() > Duration::from_secs(590));
    }

    // -- a pick end to end, against a TONE3000 played here ----------------------------------------------------------

    /// What the fake TONE3000 serves.
    #[derive(Clone)]
    struct Fake {
        file: Vec<u8>,
        /// Content-Length sent with the file.
        sized: bool,
        /// The token endpoint answers this status.
        token_status: u16,
    }

    /// A tiny HTTP server: the token, the tone, its models (architecture 2 refused, as a stand-in for any refusal),
    /// the file. Returns its base URL (the API's) and the requests it saw.
    fn serve(f: Fake) -> (String, Arc<Mutex<Vec<String>>>) {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let seen2 = seen.clone();
        std::thread::spawn(move || {
            for s in l.incoming() {
                let Ok(mut s) = s else { continue };
                let mut rd = BufReader::new(s.try_clone().unwrap());
                let mut line = String::new();
                if rd.read_line(&mut line).is_err() {
                    continue;
                }
                let mut len = 0usize;
                loop {
                    let mut h = String::new();
                    if rd.read_line(&mut h).is_err() || h.trim().is_empty() {
                        break;
                    }
                    if let Some(v) = h.to_ascii_lowercase().strip_prefix("content-length:") {
                        len = v.trim().parse().unwrap_or(0);
                    }
                }
                let mut body = vec![0u8; len];
                let _ = rd.read_exact(&mut body);
                let target = line.split_whitespace().nth(1).unwrap_or("").to_string();
                seen2
                    .lock()
                    .unwrap()
                    .push(format!("{target} {}", String::from_utf8_lossy(&body)));
                let base = format!("http://127.0.0.1:{port}");
                let (status, ty, out): (u16, &str, Vec<u8>) = if target.starts_with("/api/v1/oauth/token") {
                    (
                        f.token_status,
                        "application/json",
                        br#"{"access_token":"tok"}"#.to_vec(),
                    )
                } else if target.starts_with("/api/v1/tones/42") {
                    (
                        200,
                        "application/json",
                        br#"{"id":42,"title":"Plexi Crunch","license":"cc-by","gear":"amp","user":{"username":"jo"}}"#
                            .to_vec(),
                    )
                } else if target.starts_with("/api/v1/models") {
                    if target.contains("architecture=2") {
                        (400, "application/json", br#"{"error":"no"}"#.to_vec())
                    } else if target.contains("architecture=1") {
                        (
                            200,
                            "application/json",
                            format!(
                                r#"{{"data":[{{"id":1,"name":"Plexi Crunch","size":"standard","model_url":"{base}/files/1.nam"}}]}}"#
                            )
                            .into_bytes(),
                        )
                    } else {
                        (200, "application/json", b"[]".to_vec())
                    }
                } else if target.starts_with("/files/1.nam") {
                    (200, "application/octet-stream", f.file.clone())
                } else {
                    (404, "text/plain", Vec::new())
                };
                let mut head = format!("HTTP/1.1 {status} X\r\nContent-Type: {ty}\r\nConnection: close\r\n");
                if f.sized || !target.starts_with("/files/") {
                    head.push_str(&format!("Content-Length: {}\r\n", out.len()));
                }
                head.push_str("\r\n");
                let _ = s.write_all(head.as_bytes());
                let _ = s.write_all(&out);
            }
        });
        (format!("http://127.0.0.1:{port}/api/v1"), seen)
    }

    /// The browser: comes back to the redirect with `extra` after the state (or never, when None).
    fn browser(extra: Option<&'static str>) -> Box<dyn Fn(&str) + Send> {
        Box::new(move |url: &str| {
            let Some(extra) = extra else { return };
            let q = url.split_once('?').unwrap().1;
            let get = |k: &str| {
                q.split('&')
                    .find_map(|kv| kv.strip_prefix(&format!("{k}=")))
                    .map(dec)
                    .unwrap()
            };
            let (redirect, state) = (get("redirect_uri"), get("state"));
            std::thread::spawn(move || {
                let hp = redirect.trim_start_matches("http://");
                let (host, path) = hp.split_once('/').unwrap();
                let mut s = std::net::TcpStream::connect(host).unwrap();
                write!(s, "GET /{path}?state={state}&{extra} HTTP/1.1\r\nHost: x\r\n\r\n").unwrap();
                let mut r = String::new();
                let _ = s.read_to_string(&mut r);
            });
        })
    }

    fn env(api: String, open: Box<dyn Fn(&str) + Send>, measured: Arc<AtomicBool>, wait: Duration) -> Env {
        Env {
            api,
            open,
            add: Box::new(|kind, name, bytes, meta| {
                if bytes.starts_with(b"{") {
                    Ok(Item {
                        name: name.into(),
                        kind,
                        ..meta
                    })
                } else {
                    Err("not a NAM capture".into())
                }
            }),
            measure: Box::new(move || measured.store(true, Ordering::Relaxed)),
            wait,
        }
    }

    /// Waits for the pick to end, keeping every phase it was seen in.
    fn finish(pk: &Pick) -> Vec<Phase> {
        let t = Instant::now();
        let mut seen: Vec<Phase> = Vec::new();
        loop {
            let ph = pk.phase();
            if seen.last() != Some(&ph) {
                seen.push(ph.clone());
            }
            if ph.finished() {
                return seen;
            }
            assert!(t.elapsed() < Duration::from_secs(20), "stuck in {seen:?}");
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    fn file() -> Vec<u8> {
        let mut f = br#"{"version":"0.5.4","weights":["#.to_vec();
        f.extend(std::iter::repeat_n(b'0', 200_000));
        f.extend(b"]}");
        f
    }

    #[test]
    fn a_pick_end_to_end() {
        let f = file();
        let (api, seen) = serve(Fake {
            file: f.clone(),
            sized: true,
            token_status: 200,
        });
        let measured = Arc::new(AtomicBool::new(false));
        let unit = super::super::suggest::UNITS
            .iter()
            .find(|u| u.kind == Kind::Nam)
            .unwrap();
        let pk = start_with(
            "t3k_pub_test".into(),
            Kind::Nam,
            Some(unit),
            env(
                api,
                browser(Some("code=thecode&tone_id=42")),
                measured.clone(),
                WAIT,
            ),
        );
        let phases = finish(&pk);
        let Some(Phase::Done(it)) = phases.last() else {
            panic!("{phases:?}")
        };
        assert_eq!(it.name, "Plexi Crunch");
        assert_eq!(it.source, "tone3000");
        assert_eq!(it.size.as_deref(), Some("standard"));
        assert_eq!(
            it.skin.as_deref(),
            Some(unit.skin),
            "marked as the suggestion's unit"
        );
        assert!(measured.load(Ordering::Relaxed), "measured before done");
        assert_eq!(pk.title().as_deref(), Some("Plexi Crunch"));
        assert_eq!(
            pk.progress(),
            Progress {
                got: f.len() as u64,
                total: Some(f.len() as u64)
            }
        );
        // the phases in order (a fast one can be missed by the poll, never out of order)
        let order = |p: &Phase| match p {
            Phase::InBrowser => 0,
            Phase::SigningIn => 1,
            Phase::Reading => 2,
            Phase::Downloading { .. } => 3,
            Phase::Measuring(_) => 4,
            _ => 5,
        };
        assert!(
            phases.windows(2).all(|w| order(&w[0]) < order(&w[1])),
            "{phases:?}"
        );
        assert_eq!(phases[0], Phase::InBrowser);
        let seen = seen.lock().unwrap().clone();
        // the PKCE exchange, and the models asked for one architecture at a time (D158)
        assert!(seen.iter().any(|r| r.starts_with("/api/v1/oauth/token")
            && r.contains("code=thecode")
            && r.contains("code_verifier=")));
        for a in ["architecture=1 ", "architecture=2 ", "architecture=custom "] {
            assert!(
                seen.iter()
                    .any(|r| r.starts_with("/api/v1/models") && r.contains(a)),
                "{a}: {seen:?}"
            );
        }
        assert!(!seen.iter().any(|r| r.contains("1_2")), "{seen:?}");
    }

    #[test]
    fn a_file_with_no_size_still_comes() {
        let f = file();
        let (api, _) = serve(Fake {
            file: f.clone(),
            sized: false,
            token_status: 200,
        });
        let pk = start_with(
            "k".into(),
            Kind::Nam,
            None,
            env(api, browser(Some("code=c&tone_id=42")), Arc::default(), WAIT),
        );
        let phases = finish(&pk);
        assert!(matches!(phases.last(), Some(Phase::Done(_))), "{phases:?}");
        assert_eq!(pk.progress().total, None);
        assert_eq!(pk.progress().got, f.len() as u64);
    }

    #[test]
    fn a_file_that_does_not_run_is_invalid() {
        let (api, _) = serve(Fake {
            file: b"<html>not a capture</html>".to_vec(),
            sized: true,
            token_status: 200,
        });
        let pk = start_with(
            "k".into(),
            Kind::Nam,
            None,
            env(api, browser(Some("code=c&tone_id=42")), Arc::default(), WAIT),
        );
        let phases = finish(&pk);
        assert!(
            matches!(phases.last(), Some(Phase::Failed(Why::Invalid, e)) if e.contains("runs here")),
            "{phases:?}"
        );
    }

    #[test]
    fn a_refused_sign_in_says_so() {
        let (api, _) = serve(Fake {
            file: file(),
            sized: true,
            token_status: 401,
        });
        let pk = start_with(
            "k".into(),
            Kind::Nam,
            None,
            env(api, browser(Some("code=c&tone_id=42")), Arc::default(), WAIT),
        );
        let phases = finish(&pk);
        assert!(
            matches!(phases.last(), Some(Phase::Failed(Why::Site, _))),
            "{phases:?}"
        );
    }

    #[test]
    fn no_network_is_a_network_failure() {
        // (a port nothing listens on)
        let port = TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let pk = start_with(
            "k".into(),
            Kind::Nam,
            None,
            env(
                format!("http://127.0.0.1:{port}/api/v1"),
                browser(Some("code=c&tone_id=42")),
                Arc::default(),
                WAIT,
            ),
        );
        let phases = finish(&pk);
        assert!(
            matches!(phases.last(), Some(Phase::Failed(Why::Network, _))),
            "{phases:?}"
        );
    }

    #[test]
    fn cancelled_on_the_page_or_here_or_timed_out() {
        let (api, _) = serve(Fake {
            file: file(),
            sized: true,
            token_status: 200,
        });
        // on TONE3000's page
        let pk = start_with(
            "k".into(),
            Kind::Nam,
            None,
            env(api.clone(), browser(Some("canceled=true")), Arc::default(), WAIT),
        );
        assert_eq!(finish(&pk).last(), Some(&Phase::Cancelled { in_browser: true }));
        // TONE3000 said no
        let pk = start_with(
            "k".into(),
            Kind::Nam,
            None,
            env(
                api.clone(),
                browser(Some("error=invalid_request&error_description=bad+thing")),
                Arc::default(),
                WAIT,
            ),
        );
        assert!(matches!(finish(&pk).last(), Some(Phase::Failed(Why::Site, e)) if e.contains("bad thing")),);
        // here, while the browser is open
        let pk = start_with(
            "k".into(),
            Kind::Nam,
            None,
            env(api.clone(), browser(None), Arc::default(), WAIT),
        );
        std::thread::sleep(Duration::from_millis(150));
        assert_eq!(pk.phase(), Phase::InBrowser);
        pk.cancel();
        assert_eq!(finish(&pk).last(), Some(&Phase::Cancelled { in_browser: false }));
        // the browser never came back
        let pk = start_with(
            "k".into(),
            Kind::Nam,
            None,
            env(api, browser(None), Arc::default(), Duration::from_millis(300)),
        );
        assert!(matches!(
            finish(&pk).last(),
            Some(Phase::Failed(Why::TimedOut, _))
        ));
    }
}
