//! Drive mode (`QUICKRS_DRIVE=DIR`, D180): lets a script (or an agent) use the real app the way a person does,
//! without touching the owner's screen, mouse or data. The window opens off-screen and doesn't take focus; input
//! goes straight into egui, not through the OS.
//!
//! The driver appends commands to `DIR/cmd.txt`, one per line, and reads `DIR/log.txt`: each command gets a line
//! `ok <n> <command>` once done (so the driver waits for it). Coordinates are window pixels, as in the shots.
//!
//!   click X Y        move there, press, release (on separate frames, as a hand does)
//!   move X Y         hover
//!   scroll DY        mouse wheel at the pointer, in points (negative scrolls down)
//!   key NAME         press and release a key (egui's names: Enter, Escape, Tab, F2, ArrowDown, A, ...)
//!   text STRING      type the text
//!   wait MS          let the app run that long
//!   shot NAME        save the window as DIR/NAME.png
//!
//! File dialogs don't open: the next line of `DIR/pick.txt` is taken as the picked path (none: cancelled), and a
//! `dialog <title> -> <path>` line is logged. Web pages don't open: `url <address>` is logged instead.
//! It refuses to start without QUICKRS_DATA, so it never reads or writes the owner's own settings.

use std::collections::VecDeque;
use std::io::Write;
use std::path::PathBuf;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use eframe::egui;
use egui::{Event, Modifiers, PointerButton, Pos2};

/// The drive folder, when drive mode is on.
pub fn dir() -> Option<&'static PathBuf> {
    static DIR: OnceLock<Option<PathBuf>> = OnceLock::new();
    DIR.get_or_init(|| std::env::var_os("QUICKRS_DRIVE").map(PathBuf::from))
        .as_ref()
}

pub fn on() -> bool {
    dir().is_some()
}

/// Appends a line to the log.
pub fn log(line: &str) {
    let Some(d) = dir() else { return };
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(d.join("log.txt"))
    {
        let _ = writeln!(f, "{line}");
    }
}

/// In drive mode, a web page is logged rather than opened: true when it was.
pub fn intercept_url(url: &str) -> bool {
    if on() {
        log(&format!("url {url}"));
        true
    } else {
        false
    }
}

/// In drive mode, a file dialog's answer: the next line of pick.txt (taken off it), or None for cancelled.
/// `None` outside drive mode (the real dialog opens).
pub fn pick(title: &str) -> Option<Option<PathBuf>> {
    let d = dir()?;
    let path = d.join("pick.txt");
    let text = std::fs::read_to_string(&path).unwrap_or_default();
    let mut lines = text.lines().filter(|l| !l.trim().is_empty());
    let got = lines.next().map(|l| PathBuf::from(l.trim()));
    let rest: Vec<&str> = lines.collect();
    let _ = std::fs::write(&path, rest.join("\n"));
    log(&format!(
        "dialog {title} -> {}",
        got.as_ref()
            .map_or("(cancelled)".to_string(), |p| p.display().to_string())
    ));
    Some(got)
}

/// Checks drive mode can start (QUICKRS_DATA set), before the window opens.
pub fn check() -> anyhow::Result<()> {
    if let Some(d) = dir() {
        if std::env::var_os("QUICKRS_DATA").is_none() {
            anyhow::bail!(
                "QUICKRS_DRIVE needs QUICKRS_DATA (a scratch data folder), so the owner's data stays out"
            );
        }
        std::fs::create_dir_all(d)?;
        let _ = std::fs::write(d.join("log.txt"), "");
        if !d.join("cmd.txt").exists() {
            let _ = std::fs::write(d.join("cmd.txt"), "");
        }
    }
    Ok(())
}

enum Step {
    Events(Vec<Event>),
    Until(Instant),
    Shot(String),
    Done(String),
}

pub struct Driver {
    read: usize,
    n: usize,
    steps: VecDeque<Step>,
    pos: Pos2,
    shooting: Option<String>,
    last_poll: Instant,
}

impl Driver {
    pub fn new() -> Option<Driver> {
        on().then(|| Driver {
            read: 0,
            n: 0,
            steps: VecDeque::new(),
            pos: Pos2::new(1.0, 1.0),
            shooting: None,
            last_poll: Instant::now() - Duration::from_secs(1),
        })
    }

    fn poll(&mut self, ppp: f32) {
        if self.last_poll.elapsed() < Duration::from_millis(100) {
            return;
        }
        self.last_poll = Instant::now();
        let Some(d) = dir() else { return };
        let text = std::fs::read_to_string(d.join("cmd.txt")).unwrap_or_default();
        let lines: Vec<&str> = text.lines().collect();
        // (a line still being written has no newline yet: wait for it)
        let complete = if text.ends_with('\n') {
            lines.len()
        } else {
            lines.len().saturating_sub(1)
        };
        for line in lines.iter().take(complete).skip(self.read) {
            self.parse(line.trim(), ppp);
        }
        self.read = self.read.max(complete);
    }

    fn parse(&mut self, line: &str, ppp: f32) {
        if line.is_empty() || line.starts_with('#') {
            return;
        }
        self.n += 1;
        let done = Step::Done(format!("ok {} {line}", self.n));
        let (cmd, rest) = line.split_once(' ').unwrap_or((line, ""));
        let nums: Vec<f32> = rest.split_whitespace().filter_map(|s| s.parse().ok()).collect();
        let at = |i: usize| nums.get(i).copied().unwrap_or(0.0);
        let p = Pos2::new(at(0) / ppp, at(1) / ppp);
        let m = Modifiers::NONE;
        match cmd {
            "click" => {
                self.steps.push_back(Step::Events(vec![Event::PointerMoved(p)]));
                self.steps.push_back(Step::Events(vec![Event::PointerButton {
                    pos: p,
                    button: PointerButton::Primary,
                    pressed: true,
                    modifiers: m,
                }]));
                self.steps.push_back(Step::Events(vec![Event::PointerButton {
                    pos: p,
                    button: PointerButton::Primary,
                    pressed: false,
                    modifiers: m,
                }]));
                self.steps.push_back(Step::Until(Instant::now()));
            }
            "move" => self.steps.push_back(Step::Events(vec![Event::PointerMoved(p)])),
            "scroll" => self.steps.push_back(Step::Events(vec![Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: egui::vec2(0.0, at(0)),
                phase: egui::TouchPhase::Move,
                modifiers: m,
            }])),
            "key" => match egui::Key::from_name(rest.trim()) {
                Some(key) => {
                    for pressed in [true, false] {
                        self.steps.push_back(Step::Events(vec![Event::Key {
                            key,
                            physical_key: None,
                            pressed,
                            repeat: false,
                            modifiers: m,
                        }]));
                    }
                }
                None => {
                    log(&format!("error {} unknown key {rest}", self.n));
                    return;
                }
            },
            "text" => self
                .steps
                .push_back(Step::Events(vec![Event::Text(rest.to_string())])),
            "wait" => self.steps.push_back(Step::Until(
                Instant::now() + Duration::from_millis(at(0).max(0.0) as u64),
            )),
            "shot" => self.steps.push_back(Step::Shot(rest.trim().to_string())),
            _ => {
                log(&format!("error {} unknown command {line}", self.n));
                return;
            }
        }
        self.steps.push_back(done);
    }

    /// Before each frame: the next step's input into the frame.
    pub fn input(&mut self, ctx: &egui::Context, raw: &mut egui::RawInput) {
        self.poll(ctx.pixels_per_point());
        // a screenshot being taken: save it when it arrives, nothing else meanwhile
        if let Some(name) = self.shooting.clone() {
            for ev in &raw.events {
                if let Event::Screenshot { user_data, image, .. } = ev
                    && user_data
                        .data
                        .as_ref()
                        .and_then(|d| d.downcast_ref::<String>())
                        .is_some_and(|n| *n == format!("drive:{name}"))
                {
                    let [w, h] = image.size;
                    let rgba: Vec<u8> = image
                        .pixels
                        .iter()
                        .flat_map(|c| [c.r(), c.g(), c.b(), 255])
                        .collect();
                    let path = dir().map(|d| d.join(format!("{name}.png")));
                    if let (Some(path), Some(img)) =
                        (path, image::RgbaImage::from_raw(w as u32, h as u32, rgba))
                    {
                        let _ = img.save(&path);
                    }
                    self.shooting = None;
                }
            }
            return;
        }
        while let Some(step) = self.steps.pop_front() {
            match step {
                Step::Events(evs) => {
                    for e in &evs {
                        if let Event::PointerMoved(p) = e {
                            self.pos = *p;
                        }
                    }
                    raw.events.extend(evs);
                    return;
                }
                Step::Until(t) => {
                    if Instant::now() < t {
                        self.steps.push_front(Step::Until(t));
                        return;
                    }
                }
                Step::Shot(name) => {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::new(format!(
                        "drive:{name}"
                    ))));
                    self.shooting = Some(name);
                    return;
                }
                Step::Done(line) => log(&line),
            }
        }
    }

    /// After each frame: keep frames coming while there's work, else poll the command file now and then.
    pub fn after(&self, ctx: &egui::Context) {
        if self.steps.is_empty() && self.shooting.is_none() {
            ctx.request_repaint_after(Duration::from_millis(100));
        } else {
            ctx.request_repaint();
        }
    }
}
