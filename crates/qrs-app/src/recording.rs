//! Recording what you hear (P23.9, owner request): R in the song player or the amp room starts a recording of the
//! output's whole mix (the song, the synth, your guitar through the rig), R again pauses and resumes it, Shift R
//! stops and saves it (owner request, 2026-10-29). One recorder for both pages (`qrs_audio::recorder`: the engine's
//! record tap drained to a WAV on its own thread). Files go to the recordings folder (config.json `recordings_dir`,
//! else `quickRS Recordings` in your Music folder): a song's named after it, `Reptilia - quickRS 2026-10-29
//! 14-03-22.wav`, the amp room's after the time and the rig, `quickRS 2026-10-29 14-03-22 Plexi Crunch.wav`
//! (P23.13). Leaving the page it was started on stops and saves it. While it runs the page shows `badge` (the red
//! lamp, the time, the keys); when it's saved the app shows `saved_notice` (the file's name and Open folder) over
//! whatever page is up.
//!
//! The airlock (P23.13, owner request): a recording is written to `<data>/.quickrs-cache/recording/`, and only once
//! it's stopped, its header written and the file closed (the recorder's thread joined) is it moved into the
//! recordings folder under its final name, so a file there is always whole and never held open. A recording under
//! a second is deleted. One left behind by a crash is moved there on the next start as `... (recovered).wav`.
//!
//! The song player's loop takes (D30: scored, with their JSON) are Ctrl R and go to the takes folder.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use eframe::egui;
use egui::{Align2, Color32, Pos2, Rect, Response, Sense, Ui, pos2, vec2};
use qrs_audio::Engine;
use qrs_audio::recorder::Recorder;
use qrs_ui::keys::{CapOn, keycap, keycap_width};
use qrs_ui::paint::{self, Fill};
use qrs_ui::text::{self, Style};
use qrs_ui::theme::t;
use qrs_ui::units::px;
use qrs_ui::widgets::{self, ButtonStyle};

use crate::state::AppState;

/// Which page a recording was started on (leaving it stops the recording; it names the file too).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Page {
    Player,
    Room,
}

/// What a page shows while recording.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Status {
    pub seconds: f64,
    pub paused: bool,
}

struct Active {
    take: Take,
    page: Page,
}

/// How long the "Saved" notice stays (hovering it keeps it).
const SAVED_FOR: Duration = Duration::from_secs(10);
/// A recording shorter than this is deleted, not saved (R then Shift R by mistake).
pub const MIN_SECONDS: f64 = 1.0;
/// A file in the airlock untouched for this long was left behind (a running recording rewrites its header every
/// 5 s), so a recording another quickRS is making now is never taken.
const LEFT_BEHIND_AFTER: Duration = Duration::from_secs(30);

#[derive(Default)]
pub struct Recording {
    active: Option<Active>,
    /// The last recording saved and when (the notice), how long it was.
    saved: Option<(PathBuf, f64, Instant)>,
    /// Screenshots: shown as if recording (no file, no engine).
    pub shot: Option<Status>,
}

impl Recording {
    pub fn status(&self) -> Option<Status> {
        if let Some(s) = self.shot {
            return Some(s);
        }
        self.active.as_ref().map(|a| Status {
            seconds: a.take.rec.seconds(),
            paused: a.take.rec.paused(),
        })
    }

    pub fn page(&self) -> Option<Page> {
        self.active.as_ref().map(|a| a.page)
    }

    /// Screenshots: the "Saved" notice for a made-up file.
    pub fn shot_saved(&mut self, path: PathBuf, seconds: f64) {
        self.saved = Some((path, seconds, Instant::now()));
    }

    /// Screenshots: what a shot staged goes (before the next is staged).
    pub fn clear_shot(&mut self) {
        self.shot = None;
        self.saved = None;
    }

    /// Screenshots: a recording `what` (recording, paused, saved: see `stage_for`) of 12:34 in the song player.
    pub fn stage(&mut self, st_dir: &Path, what: &str, name: &str) {
        self.stage_for(Page::Player, st_dir, what, name);
    }

    /// Screenshots: a recording `what` (recording, paused, saved) of 12:34 on `page`.
    pub fn stage_for(&mut self, page: Page, st_dir: &Path, what: &str, name: &str) {
        let seconds = 754.0;
        match what {
            "recording" => {
                self.shot = Some(Status {
                    seconds,
                    paused: false,
                })
            }
            "paused" => {
                self.shot = Some(Status {
                    seconds,
                    paused: true,
                })
            }
            "saved" => {
                let when = chrono::NaiveDate::from_ymd_opt(2026, 10, 29)
                    .and_then(|d| d.and_hms_opt(14, 3, 22))
                    .unwrap_or_default();
                self.shot_saved(st_dir.join(file_name(&when, page, name)), seconds);
            }
            _ => {}
        }
    }
}

// -- naming -----------------------------------------------------------------------------------------------------------

/// `what` made fit for a file name on Windows and a Mac: what either forbids (`<>:"/\|?*`, control characters) a
/// space, spaces run together, no dot or space at either end (Windows drops trailing ones, a leading dot hides a
/// file on a Mac), at most 80 characters. Apostrophes and letters from any language stay.
pub fn clean_name(what: &str) -> String {
    let w: String = what
        .chars()
        .map(|c| {
            if c.is_control() || r#"<>:"/\|?*"#.contains(c) {
                ' '
            } else {
                c
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let w: String = w.trim_start_matches(['.', ' ']).chars().take(80).collect();
    w.trim_end_matches(['.', ' ']).to_string()
}

/// A recording's file name. From a song: `<song> - quickRS <date> <time>.wav` (the song first, so a folder sorted
/// by name keeps a song's recordings together); from the amp room: `quickRS <date> <time> <rig>.wav`. Without a
/// name, `quickRS <date> <time>.wav`.
pub fn file_name(when: &chrono::NaiveDateTime, page: Page, what: &str) -> String {
    let w = clean_name(what);
    let stamp = when.format("%Y-%m-%d %H-%M-%S");
    match (page, w.is_empty()) {
        (_, true) => format!("quickRS {stamp}.wav"),
        (Page::Player, false) => format!("{w} - quickRS {stamp}.wav"),
        (Page::Room, false) => format!("quickRS {stamp} {w}.wav"),
    }
}

/// `dir/name`, or `name (2).wav`, `(3)`... when that's taken.
pub fn unique_path(dir: &Path, name: &str) -> PathBuf {
    let first = dir.join(name);
    if !first.exists() {
        return first;
    }
    let stem = name.strip_suffix(".wav").unwrap_or(name);
    (2..)
        .map(|k| dir.join(format!("{stem} ({k}).wav")))
        .find(|p| !p.exists())
        .unwrap_or(first)
}

/// "4:05", "1:02:03".
pub fn clock(seconds: f64) -> String {
    let s = seconds.max(0.0) as u64;
    if s >= 3600 {
        format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60)
    } else {
        format!("{}:{:02}", s / 60, s % 60)
    }
}

// -- the airlock ------------------------------------------------------------------------------------------------------

/// Where recordings are written while they run: `<data>/.quickrs-cache/recording`.
pub fn airlock(cache: &Path) -> PathBuf {
    cache.join("recording")
}

/// A recording running: written in the airlock, bound for `dir/name`.
pub struct Take {
    rec: Recorder,
    dir: PathBuf,
    name: String,
}

/// How a take ended.
#[derive(Debug)]
pub enum End {
    /// Moved into the recordings folder, closed and whole.
    Saved(qrs_audio::recorder::Saved),
    /// Under `MIN_SECONDS` (the seconds it had): deleted.
    TooShort(f64),
    /// The recorder or the move failed: what's left (in the recordings folder, or still in the airlock to be
    /// recovered on the next start) and why.
    Failed { left: Option<PathBuf>, why: String },
}

impl Take {
    /// Start recording what `engine` plays into the airlock `temp_dir`, to be saved as `dir/name`.
    pub fn start(engine: Arc<Engine>, temp_dir: &Path, dir: PathBuf, name: String) -> io::Result<Take> {
        std::fs::create_dir_all(temp_dir)?;
        let temp = unique_path(temp_dir, &name);
        match Recorder::start(engine, temp.clone()) {
            Ok(rec) => Ok(Take { rec, dir, name }),
            Err(e) => {
                let _ = std::fs::remove_file(&temp);
                Err(e)
            }
        }
    }

    /// The file being written (in the airlock).
    pub fn temp_path(&self) -> &Path {
        self.rec.path()
    }

    /// Stop: the recorder's thread joined (the file's header written and every handle on it closed), then the
    /// file moved into the recordings folder (or deleted when it's under a second).
    pub fn stop(self) -> End {
        let Take { rec, dir, name } = self;
        let temp = rec.path().to_path_buf();
        match rec.stop() {
            Ok(mut saved) => {
                if saved.seconds < MIN_SECONDS {
                    let _ = std::fs::remove_file(&temp);
                    return End::TooShort(saved.seconds);
                }
                match move_into(&temp, &dir, &name) {
                    Ok(p) => {
                        saved.path = p;
                        End::Saved(saved)
                    }
                    Err(e) => End::Failed {
                        left: temp.is_file().then_some(temp),
                        why: format!("couldn't move it to {}: {e}", dir.display()),
                    },
                }
            }
            Err(e) => {
                // (the disk failed mid-way: what was written is still saved, its header made right)
                let why = e.to_string();
                let left = match qrs_audio::recorder::repair_wav(&temp) {
                    Ok(secs) if secs >= MIN_SECONDS => Some(move_into(&temp, &dir, &name).unwrap_or(temp)),
                    _ => {
                        let _ = std::fs::remove_file(&temp);
                        None
                    }
                };
                End::Failed { left, why }
            }
        }
    }
}

/// Move a closed file `from` into `dir` as `name` (or `name (2)`...): a rename, or when the folder is on another
/// drive a copy (to a `.part` beside it, renamed when whole) and the original deleted. Returns where it went.
pub fn move_into(from: &Path, dir: &Path, name: &str) -> io::Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let to = unique_path(dir, name);
    if std::fs::rename(from, &to).is_ok() {
        return Ok(to);
    }
    let part = dir.join(format!(".{name}.part"));
    let copied = std::fs::copy(from, &part).and_then(|_| std::fs::rename(&part, &to));
    if let Err(e) = copied {
        let _ = std::fs::remove_file(&part);
        return Err(e);
    }
    let _ = std::fs::remove_file(from);
    Ok(to)
}

/// On start: recordings left in the airlock `temp_dir` by a crash (untouched for 30 s) made whole and moved into
/// `dir` as `<name> (recovered).wav`; ones under a second deleted. Returns what was recovered.
pub fn recover(temp_dir: &Path, dir: &Path) -> Vec<PathBuf> {
    recover_older_than(temp_dir, dir, LEFT_BEHIND_AFTER)
}

fn recover_older_than(temp_dir: &Path, dir: &Path, age: Duration) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let Ok(rd) = std::fs::read_dir(temp_dir) else {
        return out;
    };
    for e in rd.flatten() {
        let p = e.path();
        let is_wav = p.extension().is_some_and(|x| x.eq_ignore_ascii_case("wav"));
        let left = e
            .metadata()
            .ok()
            .filter(|m| m.is_file())
            .and_then(|m| m.modified().ok())
            .is_some_and(|t| t.elapsed().unwrap_or_default() >= age);
        if !is_wav || !left {
            continue;
        }
        match qrs_audio::recorder::repair_wav(&p) {
            Ok(secs) if secs >= MIN_SECONDS => {
                let stem = p
                    .file_stem()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_default();
                match move_into(&p, dir, &format!("{stem} (recovered).wav")) {
                    Ok(to) => {
                        crate::applog::write(&format!(
                            "recording: recovered {} ({secs:.1} s) from {}",
                            to.display(),
                            p.display()
                        ));
                        out.push(to);
                    }
                    Err(e) => crate::applog::write(&format!(
                        "recording: couldn't recover {} into {}: {e}",
                        p.display(),
                        dir.display()
                    )),
                }
            }
            Ok(_) => {
                let _ = std::fs::remove_file(&p);
            }
            Err(e) => crate::applog::write(&format!("recording: left {} ({e})", p.display())),
        }
    }
    out
}

/// On start (AppState::new): what a crash left recovered, and said (the "Saved" notice shows the last).
pub fn recover_on_start(st: &mut AppState) {
    let dir = st.config.recordings_dir(&st.data.root);
    let found = recover(&airlock(&st.data.cache()), &dir);
    if let Some(last) = found.last() {
        let what = match found.len() {
            1 => "a recording".to_string(),
            n => format!("{n} recordings"),
        };
        st.say(format!("Recovered {what} left by a crash"));
        st.rec.saved = Some((last.clone(), 0.0, Instant::now()));
    }
}

// -- the keys ---------------------------------------------------------------------------------------------------------

/// R: start recording (named after `what`, the song or the rig), or stop and save. Returns a message for the
/// page's toast (none when the "Saved" notice says it).
pub fn toggle(st: &mut AppState, page: Page, what: &str) -> Option<(String, String)> {
    if st.rec.shot.is_some() {
        return None;
    }
    if st.rec.active.is_some() {
        return stop(st);
    }
    let Some(engine) = st.audio.as_ref().map(|a| a.engine.clone()) else {
        return Some(("Can't record: ".into(), "there's no sound engine".into()));
    };
    let dir = st.config.recordings_dir(&st.data.root);
    let dir = std::path::absolute(&dir).unwrap_or(dir);
    let name = file_name(&chrono::Local::now().naive_local(), page, what);
    let temp = airlock(&st.data.cache());
    match Take::start(engine, &temp, dir, name) {
        Ok(take) => {
            st.rec.active = Some(Active { take, page });
            st.rec.saved = None;
            Some((
                "Recording what you hear  \u{b7}  ".into(),
                "R pauses, Shift R stops and saves".into(),
            ))
        }
        Err(e) => {
            crate::applog::write(&format!("recording: couldn't start in {}: {e}", temp.display()));
            Some(("Couldn't record: ".into(), format!("{e} ({})", temp.display())))
        }
    }
}

/// R: start recording, or pause / resume the one running.
pub fn press(st: &mut AppState, page: Page, what: &str) -> Option<(String, String)> {
    if st.rec.shot.is_some() || st.rec.active.is_some() {
        pause(st)
    } else {
        toggle(st, page, what)
    }
}

/// Shift R: stop and save (a hint when not recording).
pub fn finish(st: &mut AppState) -> Option<(String, String)> {
    if st.rec.shot.is_some() {
        return None;
    }
    if st.rec.active.is_none() {
        return Some(("Not recording: ".into(), "R starts".into()));
    }
    stop(st)
}

/// Pause or resume (nothing when not recording).
pub fn pause(st: &mut AppState) -> Option<(String, String)> {
    if let Some(s) = st.rec.shot.as_mut() {
        s.paused = !s.paused;
        return None;
    }
    let Some(a) = st.rec.active.as_ref() else {
        return Some(("Not recording: ".into(), "R starts".into()));
    };
    let p = !a.take.rec.paused();
    a.take.rec.set_paused(p);
    Some(if p {
        (
            "Recording paused  \u{b7}  ".into(),
            "R carries on, Shift R stops and saves".into(),
        )
    } else {
        (
            "Recording again  \u{b7}  ".into(),
            "R pauses, Shift R stops and saves".into(),
        )
    })
}

/// Stop and save (the "Saved" notice, or a message).
pub fn stop(st: &mut AppState) -> Option<(String, String)> {
    let a = st.rec.active.take()?;
    let temp = a.take.temp_path().to_path_buf();
    match a.take.stop() {
        End::Saved(saved) => {
            crate::applog::write(&format!(
                "recording: saved {} ({:.1} s{})",
                saved.path.display(),
                saved.seconds,
                if saved.lost_seconds > 0.0 {
                    format!(", {:.1} s lost to a stall", saved.lost_seconds)
                } else {
                    String::new()
                }
            ));
            st.rec.saved = Some((saved.path, saved.seconds, Instant::now()));
            saved
                .full
                .then(|| ("Recording stopped: ".into(), "a WAV file can't be longer".into()))
        }
        End::TooShort(secs) => {
            crate::applog::write(&format!("recording: {secs:.1} s, deleted ({})", temp.display()));
            Some((
                "Nothing saved: ".into(),
                "the recording was under a second".into(),
            ))
        }
        End::Failed { left, why } => {
            crate::applog::write(&format!("recording: {}: {why}", temp.display()));
            st.rec.saved = left.map(|p| (p, 0.0, Instant::now()));
            Some(("The recording didn't save properly: ".into(), why))
        }
    }
}

/// Leaving `page`: a recording started there is stopped and saved.
pub fn leaving(st: &mut AppState, page: Page) {
    st.rec.shot = None;
    if st.rec.page() == Some(page)
        && let Some((a, b)) = stop(st)
    {
        st.say(format!("{a}{b}"));
    }
}

/// Every frame: a recording that ended by itself (the size limit, a full disk) is saved and said.
pub fn poll(st: &mut AppState) -> Option<(String, String)> {
    if st.rec.active.as_ref().is_some_and(|a| a.take.rec.ended()) {
        return stop(st).or(Some(("Recording stopped".into(), String::new())));
    }
    None
}

// -- showing it in Explorer / Finder ----------------------------------------------------------------------------------

/// The systems quickRS opens folders on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Os {
    Windows,
    Mac,
    Other,
}

impl Os {
    pub const HERE: Os = if cfg!(windows) {
        Os::Windows
    } else if cfg!(target_os = "macos") {
        Os::Mac
    } else {
        Os::Other
    };
}

/// A command that shows a folder: the program and its arguments. On Windows they're `raw`: Explorer reads its own
/// command line (not the C runtime's way), so `/select,` and the quoted path must reach it exactly as built here.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OpenCmd {
    pub program: &'static str,
    pub args: Vec<String>,
    pub raw: bool,
}

/// A path as Explorer wants it: backslashes only, no `\\?\` prefix, no trailing backslash (but a drive's `C:\`).
fn explorer_path(p: &Path) -> String {
    let mut s = p.to_string_lossy().replace('/', "\\");
    if let Some(rest) = s.strip_prefix(r"\\?\UNC\") {
        s = format!(r"\\{rest}");
    } else if let Some(rest) = s.strip_prefix(r"\\?\") {
        s = rest.to_string();
    }
    while s.len() > 3 && s.ends_with('\\') {
        s.pop();
    }
    s
}

/// The command that shows `target` on `os`: with `select`, its folder with it selected (Explorer's `/select,`,
/// Finder's `open -R`); else the folder `target` itself. `target` should be absolute.
///
/// P23.13: the "Saved" notice's Open folder went to the wrong place. Its argument was `/select,C:\...\quickRS
/// Recordings\x.wav`; Rust quotes an argument holding a space whole (`"/select,C:\...x.wav"`), which Explorer
/// doesn't take apart, so it opened its default folder. Explorer wants `/select,"C:\...x.wav"`, raw.
pub fn open_cmd(os: Os, target: &Path, select: bool) -> OpenCmd {
    match os {
        Os::Windows => {
            let p = explorer_path(target);
            OpenCmd {
                program: "explorer",
                args: vec![if select {
                    format!("/select,\"{p}\"")
                } else {
                    format!("\"{p}\"")
                }],
                raw: true,
            }
        }
        Os::Mac => {
            let p = target.to_string_lossy().into_owned();
            OpenCmd {
                program: "open",
                args: if select { vec!["-R".into(), p] } else { vec![p] },
                raw: false,
            }
        }
        Os::Other => {
            let dir = if select {
                target.parent().unwrap_or(target)
            } else {
                target
            };
            OpenCmd {
                program: "xdg-open",
                args: vec![dir.to_string_lossy().into_owned()],
                raw: false,
            }
        }
    }
}

fn spawn(cmd: &OpenCmd) -> io::Result<()> {
    let mut c = std::process::Command::new(cmd.program);
    #[cfg(windows)]
    if cmd.raw {
        use std::os::windows::process::CommandExt;
        for a in &cmd.args {
            c.raw_arg(a);
        }
    } else {
        c.args(&cmd.args);
    }
    #[cfg(not(windows))]
    c.args(&cmd.args);
    c.spawn().map(|_| ())
}

/// Show a saved file in its folder, selected (Explorer, Finder); its folder when the file's gone.
pub fn reveal(file: &Path) {
    let file = std::path::absolute(file).unwrap_or_else(|_| file.to_path_buf());
    if file.is_file() {
        if let Err(e) = spawn(&open_cmd(Os::HERE, &file, true)) {
            crate::applog::write(&format!("recording: couldn't show {}: {e}", file.display()));
        }
    } else if let Some(dir) = file.parent() {
        open_dir(dir);
    }
}

/// Open a folder (made first if it isn't there yet, so the Directories row's Open shows the recordings folder
/// itself, not the one it would be made in).
pub fn open_dir(dir: &Path) {
    let dir = std::path::absolute(dir).unwrap_or_else(|_| dir.to_path_buf());
    let _ = std::fs::create_dir_all(&dir);
    if let Err(e) = spawn(&open_cmd(Os::HERE, &dir, false)) {
        crate::applog::write(&format!("recording: couldn't open {}: {e}", dir.display()));
    }
}

// -- drawing ----------------------------------------------------------------------------------------------------------

/// The badge while recording, from `left` (its left edge, vertically centred there): the red lamp, REC and the
/// time (PAUSED in amber), then the keys: "R pause · Shift R stop and save". A click on it stops and saves.
pub fn badge(ui: &Ui, left: Pos2, s: Status) -> Response {
    badge_sized(ui, left, s, false)
}

/// `badge`, or (`short`) its lamp, word and time alone, where the row has no room for the keys (P37.4b).
pub fn badge_sized(ui: &Ui, left: Pos2, s: Status, short: bool) -> Response {
    let th = t();
    let p = ui.painter();
    let ctx = ui.ctx();
    let col = if s.paused { th.accent_ink } else { th.miss };
    let word = if s.paused { "PAUSED" } else { "REC" };
    let label = Style::caps(11.0, col);
    let time = Style::new(15.0, 800, th.text);
    let hint = Style::new(12.0, 600, th.caption);
    let ww = text::width(ctx, word, label);
    let h = px(26.0);
    let lamp_d = 12.0;
    let second = if s.paused { "resume" } else { "pause" };
    let hints: [(&[&str], &str); 2] = [(&["R"], second), (&["Shift", "R"], "stop and save")];
    let (main_w, whole) = widths(ctx, s);
    let w = if short { main_w } else { whole };
    let r = Rect::from_min_size(pos2(left.x, left.y - h / 2.0), vec2(w, h));
    let resp = widgets::interact(
        ui,
        Rect::from_min_size(r.min, vec2(main_w, h)),
        "rec-badge",
        Sense::click(),
    );
    paint::inset(p, r, Some(h / 2.0), 2.0, Fill::Base);
    p.rect_stroke(
        r.shrink(1.0),
        h / 2.0,
        egui::Stroke::new(1.5, col.gamma_multiply(if resp.hovered() { 0.9 } else { 0.55 })),
        egui::StrokeKind::Inside,
    );
    if resp.hovered() {
        ctx.set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    let cy = r.center().y;
    let lc = pos2(r.min.x + px(12.0) + px(lamp_d) / 2.0, cy);
    // the lamp: lit (and breathing) while recording, half-lit when paused
    let now = ctx.input(|i| i.time);
    let lit = if s.paused {
        0.35
    } else {
        0.8 + 0.2 * (now * std::f64::consts::TAU / 1.6).sin().abs() as f32
    };
    qrs_ui::keys::jewel_lamp(p, lc, lamp_d, lit, false);
    if !s.paused {
        paint::glow(
            p,
            Rect::from_center_size(lc, vec2(px(lamp_d), px(lamp_d))),
            col,
            10.0,
            Some(0.5),
            None,
        );
    }
    let mut x = lc.x + px(lamp_d) / 2.0 + px(8.0);
    text::draw(p, word, pos2(x, cy), Align2::LEFT_CENTER, label, None);
    x += ww + px(7.0);
    text::draw(p, &clock(s.seconds), pos2(x, cy), Align2::LEFT_CENTER, time, None);
    if short {
        if !s.paused {
            ctx.request_repaint_after(Duration::from_millis(100));
        }
        return resp;
    }
    // the divider, then the keys
    let dx = r.min.x + main_w;
    p.line_segment(
        [pos2(dx, r.min.y + px(7.0)), pos2(dx, r.max.y - px(7.0))],
        egui::Stroke::new(1.0, th.divider),
    );
    let mut x = dx + px(12.0);
    for (i, (keys, what)) in hints.iter().enumerate() {
        if i > 0 {
            x += px(8.0);
            let d = text::draw(p, "\u{b7}", pos2(x, cy), Align2::LEFT_CENTER, hint, None);
            x = d.max.x + px(8.0);
        }
        for k in keys.iter() {
            let kr = keycap(p, k, pos2(x, cy), Align2::LEFT_CENTER, 10.0, CapOn::Material);
            x = kr.max.x + px(3.0);
        }
        let d = text::draw(p, what, pos2(x + px(3.0), cy), Align2::LEFT_CENTER, hint, None);
        x = d.max.x;
    }
    if !s.paused {
        // (the time and the lamp move)
        ctx.request_repaint_after(Duration::from_millis(100));
    }
    resp
}

/// The badge's width (to place it from the right or centre it).
pub fn badge_width(ui: &Ui, s: Status) -> f32 {
    widths(ui.ctx(), s).1
}

/// The badge's widths: short (lamp, word, time) and whole.
pub fn badge_widths(ui: &Ui, s: Status) -> (f32, f32) {
    widths(ui.ctx(), s)
}

/// The badge's clickable part (lamp, word, time) and its whole width.
fn widths(ctx: &egui::Context, s: Status) -> (f32, f32) {
    let th = t();
    let word = if s.paused { "PAUSED" } else { "REC" };
    let label = Style::caps(11.0, th.miss);
    let time = Style::new(15.0, 800, th.text);
    let hint = Style::new(12.0, 600, th.caption);
    let cap = |k: &str| keycap_width(ctx, k, 10.0) + px(3.0);
    let second = if s.paused { "resume" } else { "pause" };
    let hw = cap("R")
        + px(3.0)
        + text::width(ctx, second, hint)
        + px(8.0)
        + text::width(ctx, "\u{b7}", hint)
        + px(8.0)
        + cap("Shift")
        + cap("R")
        + px(3.0)
        + text::width(ctx, "stop and save", hint);
    let main_w = px(12.0)
        + px(12.0)
        + px(8.0)
        + text::width(ctx, word, label)
        + px(7.0)
        + text::width(ctx, &clock(s.seconds), time)
        + px(12.0);
    (main_w, main_w + px(12.0) + hw + px(12.0))
}

/// The "Saved" notice over the page (bottom centre of `area`, above `bottom_gap`): the file's name, how long, the
/// folder, and Open folder (a click anywhere on it opens the folder too). Gone after 10 s unless hovered.
pub fn saved_notice(ui: &Ui, st: &mut AppState, area: Rect, bottom_gap: f32) {
    let Some((path, secs, at)) = st.rec.saved.clone() else {
        return;
    };
    let th = t();
    let ctx = ui.ctx().clone();
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let dir = path.parent().map(|d| d.display().to_string()).unwrap_or_default();
    let title = Style::new(15.0, 800, th.text);
    let cap = Style::new(12.0, 500, th.caption);
    let bw = widgets::button_width(&ctx, "Open folder", "", 13.0, false);
    let text_w = text::width(&ctx, &format!("Saved: {name}"), title)
        .max(text::width(&ctx, &dir, cap) + px(60.0))
        .min(area.width() - px(160.0) - bw);
    let w = px(20.0) + text_w + px(16.0) + bw + px(16.0);
    let h = px(64.0);
    let r = Rect::from_min_size(
        pos2(area.center().x - w / 2.0, area.max.y - bottom_gap - h),
        vec2(w, h),
    );
    let card = widgets::interact(ui, r, "rec-saved", Sense::click());
    if at.elapsed() > SAVED_FOR && !card.hovered() {
        st.rec.saved = None;
        return;
    }
    ctx.request_repaint_after(Duration::from_millis(250));
    let p = ui.painter();
    paint::raised(p, r, Some(16.0), 4.0, Fill::Base, 0.0);
    let x = r.min.x + px(20.0);
    text::draw(
        p,
        &format!("Saved: {name}"),
        pos2(x, r.min.y + px(22.0)),
        Align2::LEFT_CENTER,
        title,
        Some(text_w),
    );
    let sub = if secs > 0.0 {
        format!("{}  \u{b7}  {dir}", clock(secs))
    } else {
        dir
    };
    text::draw(
        p,
        &sub,
        pos2(x, r.min.y + px(44.0)),
        Align2::LEFT_CENTER,
        cap,
        Some(text_w),
    );
    let b = Rect::from_min_size(
        pos2(r.max.x - px(16.0) - bw, r.center().y - px(18.0)),
        vec2(bw, px(36.0)),
    );
    let open = widgets::button(
        ui,
        b,
        "rec-open",
        "Open folder",
        "",
        ButtonStyle {
            primary: true,
            size: 13.0,
            r: Some(12.0),
            ..Default::default()
        },
    );
    if card.hovered() {
        ctx.set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    if open.clicked() || card.clicked() {
        reveal(&path);
        st.rec.saved = None;
    }
}

/// A plain red dot (the Record key's icon in the room).
pub fn icon_dot(p: &egui::Painter, c: Pos2, s: f32, col: Color32) {
    p.circle_filled(c, px(s * 0.32), col);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn when() -> chrono::NaiveDateTime {
        chrono::NaiveDate::from_ymd_opt(2026, 10, 29)
            .unwrap()
            .and_hms_opt(14, 3, 22)
            .unwrap()
    }

    /// A folder of its own under the system's temp folder (with a space and an apostrophe in it).
    fn scratch(what: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("qrs rec's {what} {}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn a_songs_recording_starts_with_the_song_the_rooms_with_the_time() {
        let w = when();
        assert_eq!(
            file_name(&w, Page::Player, "Reptilia"),
            "Reptilia - quickRS 2026-10-29 14-03-22.wav"
        );
        assert_eq!(
            file_name(&w, Page::Room, "Plexi Crunch"),
            "quickRS 2026-10-29 14-03-22 Plexi Crunch.wav"
        );
        // apostrophes and any language's letters stay
        assert_eq!(
            file_name(&w, Page::Player, "Knockin' on Heaven's Door"),
            "Knockin' on Heaven's Door - quickRS 2026-10-29 14-03-22.wav"
        );
        assert_eq!(
            file_name(&w, Page::Player, "Sigur Rós – Hoppípolla 日本"),
            "Sigur Rós – Hoppípolla 日本 - quickRS 2026-10-29 14-03-22.wav"
        );
        // what Windows or a Mac forbids goes; a dot or space at either end too
        assert_eq!(
            file_name(&w, Page::Player, "AC/DC: Back in Black?  "),
            "AC DC Back in Black - quickRS 2026-10-29 14-03-22.wav"
        );
        assert_eq!(
            file_name(&w, Page::Room, "a<b>c\"d|e*f\\g\tz"),
            "quickRS 2026-10-29 14-03-22 a b c d e f g z.wav"
        );
        assert_eq!(
            file_name(&w, Page::Player, "...Lead..."),
            "Lead - quickRS 2026-10-29 14-03-22.wav"
        );
        assert_eq!(
            file_name(&w, Page::Player, " "),
            "quickRS 2026-10-29 14-03-22.wav"
        );
        assert_eq!(file_name(&w, Page::Room, ""), "quickRS 2026-10-29 14-03-22.wav");
        let long = "x".repeat(200);
        assert_eq!(
            file_name(&w, Page::Room, &long).len(),
            "quickRS 2026-10-29 14-03-22 .wav".len() + 80
        );
    }

    #[test]
    fn a_taken_name_gets_a_number() {
        let dir = scratch("names");
        let a = unique_path(&dir, "Reptilia - quickRS x.wav");
        assert_eq!(a, dir.join("Reptilia - quickRS x.wav"));
        std::fs::write(&a, b"").unwrap();
        let b = unique_path(&dir, "Reptilia - quickRS x.wav");
        assert_eq!(b, dir.join("Reptilia - quickRS x (2).wav"));
        std::fs::write(&b, b"").unwrap();
        assert_eq!(
            unique_path(&dir, "Reptilia - quickRS x.wav"),
            dir.join("Reptilia - quickRS x (3).wav")
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_clock_reads_minutes_then_hours() {
        assert_eq!(clock(0.4), "0:00");
        assert_eq!(clock(65.9), "1:05");
        assert_eq!(clock(3723.0), "1:02:03");
    }

    #[test]
    fn explorer_gets_select_and_the_path_quoted_apart() {
        let f = Path::new(
            r"C:\Users\arpan\Music\quickRS Recordings\Knockin' on Heaven's Door - quickRS 2026-10-29 14-03-22.wav",
        );
        let c = open_cmd(Os::Windows, f, true);
        assert_eq!(c.program, "explorer");
        assert!(c.raw, "Explorer's argument must reach it exactly as built");
        assert_eq!(
            c.args,
            vec![format!(r#"/select,"{}""#, f.display())],
            "one argument: /select, then the whole path in quotes"
        );
        // non-ASCII kept as it is
        let f = Path::new(r"D:\Música\Sigur Rós\日本 - quickRS.wav");
        assert_eq!(
            open_cmd(Os::Windows, f, true).args,
            vec![r#"/select,"D:\Música\Sigur Rós\日本 - quickRS.wav""#.to_string()]
        );
        // forward slashes (a config.json written by hand) and the verbatim prefix are what Explorer can't read
        assert_eq!(
            open_cmd(Os::Windows, Path::new("C:/My Recs/a b.wav"), true).args,
            vec![r#"/select,"C:\My Recs\a b.wav""#.to_string()]
        );
        assert_eq!(
            open_cmd(Os::Windows, Path::new(r"\\?\C:\My Recs\a.wav"), true).args,
            vec![r#"/select,"C:\My Recs\a.wav""#.to_string()]
        );
        assert_eq!(
            open_cmd(Os::Windows, Path::new(r"\\?\UNC\nas\share\a.wav"), true).args,
            vec![r#"/select,"\\nas\share\a.wav""#.to_string()]
        );
        // a folder: just the folder, quoted, without a trailing backslash (a drive's root keeps its own)
        assert_eq!(
            open_cmd(
                Os::Windows,
                Path::new(r"C:\Users\arpan\Music\quickRS Recordings\"),
                false
            )
            .args,
            vec![r#""C:\Users\arpan\Music\quickRS Recordings""#.to_string()]
        );
        assert_eq!(
            open_cmd(Os::Windows, Path::new(r"E:\"), false).args,
            vec![r#""E:\""#.to_string()]
        );
    }

    #[test]
    fn finder_and_xdg_get_the_path_as_one_argument() {
        let f = Path::new("/Users/me/Music/quickRS Recordings/Knockin' on Heaven's Door - quickRS.wav");
        let c = open_cmd(Os::Mac, f, true);
        assert_eq!((c.program, c.raw), ("open", false));
        assert_eq!(c.args, vec!["-R".to_string(), f.to_string_lossy().into_owned()]);
        let d = Path::new("/Users/me/Music/quickRS Recordings");
        assert_eq!(
            open_cmd(Os::Mac, d, false).args,
            vec![d.to_string_lossy().into_owned()]
        );
        let c = open_cmd(Os::Other, f, true);
        assert_eq!(c.program, "xdg-open");
        assert_eq!(c.args, vec![d.to_string_lossy().into_owned()], "its folder");
    }

    fn null_engine() -> Arc<Engine> {
        use qrs_audio::backend::{Backend, BufferSize, DeviceId};
        let e = Arc::new(Engine::new());
        e.open(qrs_audio::Route {
            input: None,
            output: Some(DeviceId {
                backend: Backend::Null,
                id: "null".into(),
            }),
            exclusive: false,
            buffer: BufferSize::Frames(256),
            base_channel: 0,
        });
        assert!(e.settle(Duration::from_secs(10)));
        e
    }

    fn files_in(dir: &Path) -> Vec<String> {
        let mut v: Vec<String> = std::fs::read_dir(dir)
            .map(|rd| {
                rd.flatten()
                    .map(|e| e.file_name().to_string_lossy().into_owned())
                    .collect()
            })
            .unwrap_or_default();
        v.sort();
        v
    }

    /// Nothing holds the file: it opens for writing with no sharing at all (Windows), and it can be renamed.
    fn not_locked(p: &Path) {
        let mut o = std::fs::OpenOptions::new();
        o.read(true).write(true);
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            o.share_mode(0);
        }
        drop(o.open(p).expect("the saved file opens for exclusive writing"));
        let moved = p.with_extension("moved.wav");
        std::fs::rename(p, &moved).expect("the saved file can be renamed");
        std::fs::rename(&moved, p).unwrap();
    }

    #[test]
    fn a_recording_is_written_aside_then_moved_in_closed() {
        let e = null_engine();
        let root = scratch("airlock");
        let (temp, dir) = (
            root.join("cache").join("recording"),
            root.join("Récordings ünd 'so'"),
        );
        let name = file_name(&when(), Page::Player, "Knockin' on Heaven's Door");
        // one already there: this one gets " (2)"
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(&name), b"older").unwrap();
        let take = Take::start(e.clone(), &temp, dir.clone(), name.clone()).unwrap();
        let written = take.temp_path().to_path_buf();
        assert!(written.starts_with(&temp), "{}", written.display());
        assert_eq!(
            files_in(&dir).len(),
            1,
            "nothing in the recordings folder while it runs"
        );
        std::thread::sleep(Duration::from_millis(1500));
        let End::Saved(saved) = take.stop() else {
            panic!("not saved")
        };
        let want = dir.join("Knockin' on Heaven's Door - quickRS 2026-10-29 14-03-22 (2).wav");
        assert_eq!(saved.path, want);
        assert!(saved.seconds >= MIN_SECONDS, "{saved:?}");
        assert!(
            files_in(&temp).is_empty(),
            "the airlock is empty: {:?}",
            files_in(&temp)
        );
        assert!(e.record_tap().is_none());
        let (rate, ch, s) = qrs_audio::decode::read_wav_i16(&std::fs::read(&saved.path).unwrap()).unwrap();
        assert_eq!((rate, ch), (48000, 2));
        assert!(((s.len() / 2) as f64 / rate as f64 - saved.seconds).abs() < 1e-6);
        not_locked(&saved.path);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_recording_under_a_second_is_deleted() {
        let e = null_engine();
        let root = scratch("short");
        let (temp, dir) = (root.join("airlock"), root.join("recs"));
        let take = Take::start(e, &temp, dir.clone(), "x.wav".into()).unwrap();
        std::thread::sleep(Duration::from_millis(200));
        assert!(matches!(take.stop(), End::TooShort(s) if s < MIN_SECONDS));
        assert!(files_in(&temp).is_empty());
        assert!(files_in(&dir).is_empty());
        let _ = std::fs::remove_dir_all(&root);
    }

    /// What the recorder writes first: a 16-bit stereo header with sizes 0 (as a crash before the first 5 s left).
    fn left_behind(path: &Path, seconds: f64) {
        let rate = 48000u32;
        let mut h = Vec::new();
        h.extend_from_slice(b"RIFF");
        h.extend_from_slice(&36u32.to_le_bytes());
        h.extend_from_slice(b"WAVEfmt ");
        h.extend_from_slice(&16u32.to_le_bytes());
        h.extend_from_slice(&1u16.to_le_bytes());
        h.extend_from_slice(&2u16.to_le_bytes());
        h.extend_from_slice(&rate.to_le_bytes());
        h.extend_from_slice(&(rate * 4).to_le_bytes());
        h.extend_from_slice(&4u16.to_le_bytes());
        h.extend_from_slice(&16u16.to_le_bytes());
        h.extend_from_slice(b"data");
        h.extend_from_slice(&0u32.to_le_bytes());
        h.extend(std::iter::repeat_n(3u8, (seconds * rate as f64) as usize * 4));
        std::fs::write(path, h).unwrap();
    }

    #[test]
    fn a_crash_leftover_is_recovered_on_start() {
        let root = scratch("recover");
        let (temp, dir) = (root.join("airlock"), root.join("recs"));
        std::fs::create_dir_all(&temp).unwrap();
        left_behind(&temp.join("Reptilia - quickRS 2026-10-29 14-03-22.wav"), 2.0);
        left_behind(&temp.join("quickRS 2026-10-29 15-00-00.wav"), 0.2);
        // just written: as a recording running in another quickRS would be, it's left alone
        assert!(recover(&temp, &dir).is_empty());
        assert_eq!(files_in(&temp).len(), 2);
        let got = recover_older_than(&temp, &dir, Duration::ZERO);
        let want = dir.join("Reptilia - quickRS 2026-10-29 14-03-22 (recovered).wav");
        assert_eq!(got, vec![want.clone()]);
        assert!(
            files_in(&temp).is_empty(),
            "the short one deleted: {:?}",
            files_in(&temp)
        );
        // its header made right: it plays, all of it
        let (rate, ch, s) = qrs_audio::decode::read_wav_i16(&std::fs::read(&want).unwrap()).unwrap();
        assert_eq!((rate, ch, s.len()), (48000, 2, 2 * 96000));
        not_locked(&want);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn moving_in_keeps_what_is_there() {
        let root = scratch("move");
        let dir = root.join("recs");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a.wav"), b"one").unwrap();
        let from = root.join("a.wav");
        std::fs::write(&from, b"two").unwrap();
        let to = move_into(&from, &dir, "a.wav").unwrap();
        assert_eq!(to, dir.join("a (2).wav"));
        assert!(!from.exists());
        assert_eq!(std::fs::read(dir.join("a.wav")).unwrap(), b"one");
        assert_eq!(std::fs::read(&to).unwrap(), b"two");
        let _ = std::fs::remove_dir_all(&root);
    }
}
