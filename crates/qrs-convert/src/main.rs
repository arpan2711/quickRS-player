//! quickRS Converter: a window for `quickrs-cli convert-folder`. Pick the folder with your Rocksmith .psarc songs
//! and where the feedpaks go, press Convert; every song becomes a .feedpak (qrs_library::feedpak_export::folder).
//! Drawn with quickRS's own look (qrs-ui: the Midnight theme, its cards, keys and type).

#![windows_subsystem = "windows"]

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use eframe::egui::{self, Align2, Color32, Pos2, Rect, Sense, Vec2, pos2, vec2};
use qrs_formats::Keys;
use qrs_library::feedpak_export::folder::{Event, Summary, convert_folder};
use qrs_ui::paint::{self, Fill};
use qrs_ui::text::{self, Style};
use qrs_ui::theme::t;
use qrs_ui::widgets::{self, ButtonStyle};

const STEAM_DLC: &str = r"C:\Program Files (x86)\Steam\steamapps\common\Rocksmith2014\dlc";
const W: f32 = 760.0;
const H: f32 = 700.0;
const PAD: f32 = 28.0;

/// What the conversion thread reports.
#[derive(Default)]
struct Progress {
    total: usize,
    done: usize,
    made: usize,
    failed: usize,
    log: Vec<(String, bool)>,
    result: Option<Result<Summary, String>>,
}

struct App {
    keys: Keys,
    keys_dir: PathBuf,
    from: String,
    to: String,
    /// The destination follows the source (a "feedpak" folder next to "dlc") until it's chosen by hand.
    to_chosen: bool,
    running: bool,
    stop: Arc<AtomicBool>,
    progress: Arc<Mutex<Progress>>,
    /// QRS_CONVERT_SHOT=<file.png>: save the window there after a few frames, then close (for the readme).
    shot: Option<(PathBuf, u32)>,
}

fn exe_dir() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(Path::to_path_buf))
        .unwrap_or_default()
}

/// Where quickRS keeps its data when it isn't portable (its keys.json can be there too).
fn appdata_dir() -> Option<PathBuf> {
    std::env::var_os("APPDATA").map(|a| PathBuf::from(a).join("quickRS"))
}

/// A "feedpak" folder next to a "dlc" folder (quickRS finds that one by itself), else one inside the folder.
fn default_to(from: &str) -> String {
    let p = PathBuf::from(from.trim().trim_matches('"'));
    if p.as_os_str().is_empty() {
        return String::new();
    }
    let next_to_dlc = p
        .file_name()
        .is_some_and(|n| n.to_string_lossy().eq_ignore_ascii_case("dlc"));
    match (next_to_dlc, p.parent()) {
        (true, Some(parent)) => parent.join("feedpak"),
        _ => p.join("feedpak"),
    }
    .display()
    .to_string()
}

fn load_keys() -> Keys {
    let mut dirs = vec![exe_dir()];
    dirs.extend(appdata_dir());
    qrs_formats::load_keys(None, &dirs).unwrap_or_default()
}

impl App {
    fn new(cc: &eframe::CreationContext<'_>) -> App {
        text::install(&cc.egui_ctx);
        // quickrs-convert.exe [songs folder] [feedpak folder] (a folder dropped on the exe's icon comes in this way)
        let mut args = std::env::args().skip(1);
        let from = args.next().unwrap_or_else(|| {
            if Path::new(STEAM_DLC).is_dir() {
                STEAM_DLC.to_string()
            } else {
                String::new()
            }
        });
        let to = args.next();
        App {
            keys: load_keys(),
            keys_dir: exe_dir(),
            to_chosen: to.is_some(),
            to: to.unwrap_or_else(|| default_to(&from)),
            from,
            running: false,
            stop: Arc::new(AtomicBool::new(false)),
            progress: Arc::new(Mutex::new(Progress::default())),
            shot: std::env::var_os("QRS_CONVERT_SHOT").map(|f| (PathBuf::from(f), 0)),
        }
    }

    fn start(&mut self, ctx: &egui::Context) {
        let (from, to) = (
            PathBuf::from(self.from.trim().trim_matches('"')),
            PathBuf::from(self.to.trim().trim_matches('"')),
        );
        *self.progress.lock().unwrap_or_else(|e| e.into_inner()) = Progress::default();
        self.stop.store(false, Ordering::Relaxed);
        self.running = true;
        let (keys, stop, progress, ctx) = (
            self.keys.clone(),
            self.stop.clone(),
            self.progress.clone(),
            ctx.clone(),
        );
        std::thread::spawn(move || {
            let on = |e: Event| {
                let mut p = progress.lock().unwrap_or_else(|e| e.into_inner());
                match e {
                    Event::Found(n) => p.total = n,
                    Event::Made(i, name) => {
                        p.done = p.done.max(i);
                        p.made += 1;
                        p.log.push((name, true));
                    }
                    Event::Skipped(i, _) => p.done = p.done.max(i),
                    Event::Failed(i, name, why) => {
                        p.done = p.done.max(i);
                        p.failed += 1;
                        p.log.push((format!("{name}: {why}"), false));
                    }
                }
                ctx.request_repaint();
            };
            let r = convert_folder(&keys, &from, &to, false, false, &stop, &on);
            progress.lock().unwrap_or_else(|e| e.into_inner()).result = Some(r);
            ctx.request_repaint();
        });
    }
}

fn caption(p: &egui::Painter, s: &str, at: Pos2) {
    text::caps(p, s, at, 12.0, t().caption, Align2::LEFT_TOP, None);
}

fn line(p: &egui::Painter, s: &str, at: Pos2, size: f32, weight: u16, color: Color32, max_w: f32) -> Rect {
    text::draw(
        p,
        s,
        at,
        Align2::LEFT_TOP,
        Style::new(size, weight, color),
        Some(max_w),
    )
}

/// Lines of `s` wrapped to `max_w`, drawn from `at`; returns the y under them.
fn para(p: &egui::Painter, s: &str, at: Pos2, size: f32, color: Color32, max_w: f32) -> f32 {
    let st = Style::new(size, 500, color);
    let mut y = at.y;
    for l in text::wrap(p.ctx(), s, st, max_w) {
        text::draw(p, &l, pos2(at.x, y), Align2::LEFT_TOP, st, None);
        y += text::line_height(p.ctx(), st) + 3.0;
    }
    y
}

fn open_folder(path: &Path) {
    let _ = std::process::Command::new("explorer").arg(path).spawn();
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let th = t();
        let full = ctx.content_rect();
        let p = ui.painter().clone();
        p.rect_filled(full, 0.0, th.base);
        let (x0, w) = (full.min.x + PAD, full.width() - 2.0 * PAD);
        let mut y = full.min.y + PAD;

        // the title, and what it does
        text::draw(
            &p,
            "Convert songs to feedpaks",
            pos2(x0, y),
            Align2::LEFT_TOP,
            Style::new(26.0, 800, th.text),
            None,
        );
        y += 40.0;
        y = para(
            &p,
            "Makes a feedpak of every Rocksmith song in a folder: one open, unencrypted file per song that quickRS plays \
             without keys. Your .psarc files are only read, never changed. Songs already converted are skipped, so run \
             it again after adding songs.",
            pos2(x0, y),
            14.0,
            th.text_2,
            w,
        ) + 16.0;

        // the keys
        let keys_ok = self.keys.psarc.is_some();
        let kr = Rect::from_min_size(pos2(x0, y), vec2(w, if keys_ok { 44.0 } else { 78.0 }));
        paint::inset(&p, kr, Some(14.0), 3.0, Fill::Base);
        paint::dot(
            &p,
            pos2(kr.min.x + 22.0, kr.min.y + 22.0),
            5.0,
            if keys_ok { th.hit } else { th.miss },
        );
        if keys_ok {
            line(
                &p,
                "Keys found: ready to convert",
                pos2(kr.min.x + 38.0, kr.min.y + 13.0),
                14.0,
                700,
                th.text,
                w - 60.0,
            );
        } else {
            line(
                &p,
                "keys.json is missing",
                pos2(kr.min.x + 38.0, kr.min.y + 13.0),
                14.0,
                700,
                th.text,
                w - 200.0,
            );
            para(
                &p,
                "Copy keys.example.json to keys.json in this program's folder and paste in Rocksmith's two keys (see README.txt), then press Check again.",
                pos2(kr.min.x + 38.0, kr.min.y + 36.0),
                12.0,
                th.text_2,
                w - 220.0,
            );
            let b1 = Rect::from_min_size(pos2(kr.max.x - 150.0, kr.min.y + 12.0), vec2(136.0, 26.0));
            if widgets::button(
                ui,
                b1,
                "open-dir",
                "Open the folder",
                "",
                ButtonStyle::default().size(12.0),
            )
            .clicked()
            {
                open_folder(&self.keys_dir);
            }
            let b2 = b1.translate(vec2(0.0, 32.0));
            if widgets::button(
                ui,
                b2,
                "check",
                "Check again",
                "",
                ButtonStyle::default().size(12.0),
            )
            .clicked()
            {
                self.keys = load_keys();
            }
        }
        y = kr.max.y + 22.0;

        // 1: the songs, 2: where they go
        let field = |ui: &mut egui::Ui,
                     n: &str,
                     title: &str,
                     hint: &str,
                     value: &mut String,
                     id: &str,
                     y: &mut f32|
         -> bool {
            caption(&p, &format!("{n}  {title}"), pos2(x0, *y));
            *y += 22.0;
            let fr = Rect::from_min_size(pos2(x0, *y), vec2(w - 130.0, 40.0));
            paint::inset(&p, fr, Some(12.0), 3.0, Fill::Base);
            let edit = egui::TextEdit::singleline(value)
                .frame(egui::Frame::NONE)
                .font(egui::FontId::new(14.0, text::family(500)))
                .text_color(th.text)
                .hint_text(hint)
                .desired_width(fr.width() - 28.0);
            let changed = ui
                .put(
                    Rect::from_min_size(fr.min + vec2(14.0, 11.0), vec2(fr.width() - 28.0, 20.0)),
                    edit,
                )
                .changed();
            let br = Rect::from_min_size(pos2(fr.max.x + 14.0, *y + 2.0), vec2(116.0, 36.0));
            let mut picked = false;
            if widgets::button(ui, br, id, "Browse…", "", ButtonStyle::default()).clicked() {
                let mut d = rfd::FileDialog::new().set_title(title);
                let cur = PathBuf::from(value.trim().trim_matches('"'));
                if cur.is_dir() {
                    d = d.set_directory(&cur);
                } else if let Some(par) = cur.parent().filter(|p| p.is_dir()) {
                    d = d.set_directory(par);
                }
                if let Some(f) = d.pick_folder() {
                    *value = f.display().to_string();
                    picked = true;
                }
            }
            *y += 52.0;
            changed || picked
        };
        if field(
            ui,
            "1",
            "Your Rocksmith songs (.psarc)",
            "the folder with your songs, e.g. ...\\Rocksmith2014\\dlc",
            &mut self.from,
            "from",
            &mut y,
        ) && !self.to_chosen
        {
            self.to = default_to(&self.from);
        }
        if field(
            ui,
            "2",
            "Where the feedpaks go",
            "a folder for the feedpaks",
            &mut self.to,
            "to",
            &mut y,
        ) {
            self.to_chosen = true;
        }
        let next_to_dlc = !self.to.is_empty()
            && self.to == default_to(&self.from)
            && default_to(&self.from).ends_with("feedpak")
            && PathBuf::from(self.from.trim())
                .file_name()
                .is_some_and(|n| n.to_string_lossy().eq_ignore_ascii_case("dlc"));
        let note = if next_to_dlc {
            "A \"feedpak\" folder next to \"dlc\" is found by quickRS by itself: nothing else to set up."
        } else {
            "Anywhere else: after converting, drag this folder onto the quickRS window once to add it."
        };
        line(&p, note, pos2(x0, y - 8.0), 12.0, 500, th.caption, w);
        y += 22.0;

        // convert / stop, and progress
        let (snap_total, snap_done, snap_made, snap_failed, result) = {
            let pr = self.progress.lock().unwrap_or_else(|e| e.into_inner());
            (pr.total, pr.done, pr.made, pr.failed, pr.result.clone())
        };
        if self.running && result.is_some() {
            self.running = false;
        }
        let from_ok = Path::new(self.from.trim().trim_matches('"')).is_dir();
        let can = keys_ok && from_ok && !self.to.trim().is_empty();
        let cr = Rect::from_min_size(pos2(x0, y), vec2(200.0, 46.0));
        if self.running {
            if widgets::button(ui, cr, "stop", "Stop", "", ButtonStyle::default().size(15.0)).clicked() {
                self.stop.store(true, Ordering::Relaxed);
            }
        } else {
            let st = ButtonStyle {
                enabled: can,
                ..ButtonStyle::primary().size(15.0)
            };
            if widgets::button(ui, cr, "go", "Convert", "", st).clicked() && can {
                self.start(&ctx);
            }
        }
        let status = if self.running {
            if snap_total == 0 {
                "Looking for songs…".to_string()
            } else {
                format!(
                    "{snap_done} of {snap_total} files  ·  {snap_made} converted  ·  {snap_failed} failed"
                )
            }
        } else {
            match &result {
                Some(Ok(s)) if self.stop.load(Ordering::Relaxed) => {
                    format!(
                        "Stopped: {} converted, {} already there, {} failed",
                        s.made, s.skipped, s.failed
                    )
                }
                Some(Ok(s)) => format!(
                    "Done: {} converted, {} already there, {} failed",
                    s.made, s.skipped, s.failed
                ),
                Some(Err(e)) => e.clone(),
                None if !keys_ok => "Add the keys first (above)".into(),
                None if !from_ok => "Pick the folder with your songs (1)".into(),
                None => "Ready".into(),
            }
        };
        let sx = cr.max.x + 22.0;
        line(&p, &status, pos2(sx, y + 4.0), 14.0, 700, th.text, w - (sx - x0));
        let bar = Rect::from_min_size(pos2(sx, y + 30.0), vec2(x0 + w - sx, 10.0));
        paint::inset(&p, bar, Some(5.0), 2.0, Fill::Base);
        let frac = if snap_total > 0 {
            snap_done as f32 / snap_total as f32
        } else if matches!(result, Some(Ok(_))) {
            1.0
        } else {
            0.0
        };
        if frac > 0.0 {
            let fill = Rect::from_min_size(bar.min, vec2((bar.width() * frac).max(10.0), bar.height()));
            paint::pill(&p, fill, th.accent);
        }
        y += 66.0;
        if matches!(result, Some(Ok(_))) && !self.running {
            let or = Rect::from_min_size(pos2(x0, y), vec2(230.0, 36.0));
            if widgets::button(
                ui,
                or,
                "open-to",
                "Open the feedpak folder",
                "",
                ButtonStyle::default(),
            )
            .clicked()
            {
                open_folder(Path::new(self.to.trim().trim_matches('"')));
            }
            y += 50.0;
        }

        // the log: the songs as they're made, failures in red
        let lr = Rect::from_min_max(pos2(x0, y), pos2(x0 + w, full.max.y - PAD));
        if lr.height() > 60.0 {
            paint::display(&p, lr, Some(16.0), 3.0);
            let pr = self.progress.lock().unwrap_or_else(|e| e.into_inner());
            let lh = 20.0;
            let rows = ((lr.height() - 24.0) / lh).floor().max(0.0) as usize;
            let clip = p.with_clip_rect(lr.shrink(6.0));
            if pr.log.is_empty() {
                line(
                    &clip,
                    "The songs show here as they're converted.",
                    pos2(lr.min.x + 18.0, lr.min.y + 14.0),
                    13.0,
                    500,
                    th.caption,
                    w - 36.0,
                );
            }
            let start = pr.log.len().saturating_sub(rows);
            for (i, (s, ok)) in pr.log[start..].iter().enumerate() {
                let c = if *ok { th.text_2 } else { th.miss };
                line(
                    &clip,
                    s,
                    pos2(lr.min.x + 18.0, lr.min.y + 12.0 + i as f32 * lh),
                    13.0,
                    500,
                    c,
                    w - 36.0,
                );
            }
        }
        let _ = ui.allocate_rect(full, Sense::hover());
        // (QRS_CONVERT_GO with a shot: convert straight away, and shoot when it's done)
        let go = std::env::var_os("QRS_CONVERT_GO").is_some();
        if go && self.shot.as_ref().is_some_and(|s| s.1 == 2) && !self.running {
            self.start(&ctx);
        }
        let finished = !go || result.is_some();
        if let Some((file, n)) = &mut self.shot {
            if *n != 5 || finished {
                *n += 1;
            }
            if *n == 6 {
                ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(Default::default()));
            }
            let got = ctx.input(|i| {
                i.raw.events.iter().find_map(|e| match e {
                    egui::Event::Screenshot { image, .. } => Some(image.clone()),
                    _ => None,
                })
            });
            if let Some(img) = got {
                let [w, h] = img.size;
                let rgba: Vec<u8> = img.pixels.iter().flat_map(|c| c.to_array()).collect();
                if let Some(buf) = image::RgbaImage::from_raw(w as u32, h as u32, rgba) {
                    let _ = buf.save(&*file);
                }
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            ctx.request_repaint();
        }
    }
}

fn icon() -> egui::IconData {
    let bytes = include_bytes!("../../../assets/icons/rsplay.png");
    match image::load_from_memory(bytes) {
        Ok(img) => {
            let img = img.to_rgba8();
            egui::IconData {
                width: img.width(),
                height: img.height(),
                rgba: img.into_raw(),
            }
        }
        Err(_) => egui::IconData::default(),
    }
}

fn main() -> eframe::Result {
    let opts = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("quickRS Converter")
            .with_inner_size(Vec2::new(W, H))
            .with_min_inner_size(Vec2::new(640.0, 600.0))
            .with_icon(icon()),
        ..Default::default()
    };
    eframe::run_native(
        "quickRS Converter",
        opts,
        Box::new(|cc| Ok(Box::new(App::new(cc)))),
    )
}
