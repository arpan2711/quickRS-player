//! The app: one window, its scale (egui zoomed so one point is one of the Python app's scale-1 pixels, D15), the
//! theme, F11, the title bar colours, and the page being shown (Home, the library pages, the gallery).

use std::path::PathBuf;

use eframe::egui;
use egui::{Event, Key, ViewportCommand};
use qrs_ui::theme::{self, ThemeName, t};

use crate::backdrop::Backdrop;
use crate::devstatus::DevStatus;
use crate::home::HomeView;
use crate::library::{LibView, Part, Req, View};
use crate::loading::Loading;
use crate::notewidget::NoteWidget;
use crate::player::{PlayerReq, PlayerView};
use crate::settings::SettingsView;
use crate::state::{AppState, AudioMode};
use crate::{chrome, gallery, shots};

#[derive(Clone, Debug, Default)]
pub struct Args {
    pub gallery: bool,
    pub screenshots: Option<PathBuf>,
    /// Physical pixels.
    pub size: Option<(u32, u32)>,
    pub theme: Option<String>,
    /// The knob style (amp, pedal, chrome, vintage), as Settings > Display sets it.
    pub knobs: Option<String>,
    /// Run the engine on no sound devices (nothing is heard).
    pub silent: bool,
    /// Open the first song whose title has this in it and play it (tests, measurements).
    pub play: Option<String>,
    /// Started by the updater (P20.1): the old app's process id, waited for.
    pub after_update: Option<u32>,
    /// A new dev build swapped in (D67): the file saying where the old one was, and what it said (main reads it).
    pub resume: Option<PathBuf>,
    pub resumed: Option<crate::devswap::Resume>,
}

impl Args {
    pub fn parse(mut it: impl Iterator<Item = String>) -> anyhow::Result<Args> {
        let mut a = Args::default();
        while let Some(arg) = it.next() {
            match arg.as_str() {
                "--gallery" => a.gallery = true,
                "--screenshots" => {
                    a.screenshots = Some(
                        it.next()
                            .ok_or_else(|| anyhow::anyhow!("--screenshots needs a folder"))?
                            .into(),
                    )
                }
                "--size" => {
                    let v = it.next().unwrap_or_default();
                    let (w, h) = v.split_once('x').ok_or_else(|| anyhow::anyhow!("--size WxH"))?;
                    a.size = Some((w.trim().parse()?, h.trim().parse()?));
                }
                "--theme" => a.theme = it.next(),
                "--knobs" => a.knobs = it.next(),
                "--silent" => a.silent = true,
                "--play" => a.play = it.next(),
                "--after-update" => a.after_update = it.next().and_then(|p| p.parse().ok()),
                "--resume" => a.resume = it.next().map(PathBuf::from),
                "-h" | "--help" => {
                    println!(
                        "quickrs [--silent] [--play TITLE] [--gallery] [--screenshots DIR [--size WxH]] [--theme midnight|graphite|daylight] [--knobs amp|pedal|chrome|vintage]"
                    );
                    std::process::exit(0);
                }
                other => anyhow::bail!("unknown argument {other:?}"),
            }
        }
        Ok(a)
    }
}

/// The Python app's scale: the window's physical size against a 1400 x 820 layout.
/// QUICKRS_FRAMES=1: frames over 12 ms written to the log, with whether the scale changed in them (a probe for
/// how resizing feels).
struct FrameTimer {
    at: std::time::Instant,
    zoom: f32,
    ctx: egui::Context,
}

impl FrameTimer {
    fn new(ctx: &egui::Context) -> Option<FrameTimer> {
        static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
        ON.get_or_init(|| std::env::var("QUICKRS_FRAMES").is_ok_and(|v| v == "1"))
            .then(|| FrameTimer {
                at: std::time::Instant::now(),
                zoom: ctx.zoom_factor(),
                ctx: ctx.clone(),
            })
    }
}

impl Drop for FrameTimer {
    fn drop(&mut self) {
        let ms = self.at.elapsed().as_secs_f64() * 1000.0;
        let (made, made_ms) = qrs_ui::paint::take_made();
        if ms > 12.0 {
            let z = self.ctx.zoom_factor();
            let px = self.ctx.content_rect().size() * self.ctx.pixels_per_point();
            crate::applog::write(&format!(
                "frame: {ms:.1} ms, {:.0} x {:.0}, {made} textures made in {made_ms:.1} ms{}",
                px.x,
                px.y,
                if (z - self.zoom).abs() > 1e-4 {
                    ", scale changed"
                } else {
                    ""
                }
            ));
        }
    }
}

/// The window's inside, in pixels (Windows: its client rect; elsewhere unknown here).
#[cfg(windows)]
fn client_pixels(hwnd: isize) -> Option<egui::Vec2> {
    use windows::Win32::Foundation::{HWND, RECT};
    use windows::Win32::UI::WindowsAndMessaging::GetClientRect;
    let mut r = RECT::default();
    unsafe { GetClientRect(HWND(hwnd as *mut _), &mut r) }.ok()?;
    Some(egui::vec2((r.right - r.left) as f32, (r.bottom - r.top) as f32))
}

#[cfg(not(windows))]
fn client_pixels(_hwnd: isize) -> Option<egui::Vec2> {
    None
}

pub fn scale_for(physical: egui::Vec2) -> f32 {
    (physical.x / 1400.0).min(physical.y / 820.0).clamp(0.7, 2.2)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Screen {
    Gallery,
    /// Home and the library pages (which one is `LibView::view`).
    Pages,
}

pub struct QuickRs {
    hwnd: Option<isize>,
    /// A dialog's frosted backdrop wasn't captured in time: the dialog goes over the page as it is until it closes.
    dlg_gave_up: bool,
    /// The song being loaded opens paused (a double-click in the library).
    open_paused: bool,
    /// ...with its last review up (Home's Carry on card).
    open_review: bool,
    styled: Option<ThemeName>,
    pub screen: Screen,
    gallery: gallery::Gallery,
    shots: Option<shots::Shots>,
    /// drive mode (QUICKRS_DRIVE): scripted input, D180
    driver: Option<crate::drive::Driver>,
    pub state: AppState,
    pub lib: LibView,
    pub home: HomeView,
    pub loading: Option<Loading>,
    pub player: Option<PlayerView>,
    /// The settings panel, floating over the page it was opened from.
    pub settings: Option<SettingsView>,
    /// The amp room (P22.2), over Home.
    pub room: Option<crate::room::RoomView>,
    backdrop: Backdrop,
    /// The library page under its dialogs, for glass dialogs (dark themes, round 3 look).
    dlg_backdrop: Backdrop,
    devstatus: DevStatus,
    /// The guitar input last seen and its tone (each input keeps its own tone, D26).
    board_input: Option<(String, String)>,
    notewidget: NoteWidget,
    shot_applied: Option<usize>,
    /// Screenshot mode: the pointer's button is held down (a pressed control's shot).
    shot_pressed: bool,
    /// Screenshot mode: settings a shot changed, as they were (put back before the next shot).
    shot_saved: Vec<(&'static str, Option<serde_json::Value>)>,
    /// Startup timings logged: the first frame, the library ready.
    timed: (bool, bool),
    /// --play: the song to open once the library is read, then start (`play_pending` until it's open).
    play: Option<String>,
    play_pending: bool,
    /// Dev builds (D67): the watch for a new build at the exe's path, whether its "waiting for a pause" notice was
    /// given; after a swap, the song to open again and then where to seek to (and play on or not).
    devswap: Option<crate::devswap::DevSwap>,
    swap_said: bool,
    /// When this app, a new build started, began closing for it.
    swap_closing: Option<std::time::Instant>,
    resume_song: Option<crate::devswap::ResumeSong>,
    resume_at: Option<(f64, bool)>,
    /// A saved lick being opened: its loop (start, end) and name, set on the player once the song is open.
    lick: Option<(f64, f64, String)>,
    /// A lesson's riff being opened (the lesson and the riff's id), handed to the player once the song is open.
    lesson: Option<(std::sync::Arc<qrs_library::Lesson>, String)>,
    /// The first-time setup over the page (P23.14), and whether the first-run check has been made.
    pub setup: Option<crate::settings::FirstSetup>,
    setup_checked: bool,
}

impl QuickRs {
    pub fn new(cc: &eframe::CreationContext<'_>, args: Args) -> QuickRs {
        qrs_ui::text::install(&cc.egui_ctx);
        cc.egui_ctx.options_mut(|o| {
            o.zoom_with_keyboard = false; // Ctrl +/- are the app's keys
        });
        let hwnd = chrome::hwnd_of(cc);
        let shots = args.screenshots.clone().map(|dir| {
            // --theme: every shot in that theme (else each in its own, mostly Midnight)
            let theme = args.theme.as_deref().map(ThemeName::parse);
            shots::Shots::new(dir, args.size.unwrap_or((1440, 900)), theme)
        });
        // screenshots never open a sound device (D13: nothing audible at night, and nothing of the owner's moves):
        // their guitar is the stand-in on the no-device backend
        // (nor does drive mode, D180: a test copy beside the owner's quickRS mustn't take the owner's interface)
        let mode = if shots.is_some() || crate::drive::on() {
            AudioMode::Shots
        } else if args.silent {
            AudioMode::Silent
        } else {
            AudioMode::Devices
        };
        let mut state = AppState::new(&cc.egui_ctx, mode);
        crate::captures::init(&state.data.root, &state.config);
        if shots.is_none() {
            // a recording a crash left in the airlock goes to the recordings folder (P23.13)
            crate::recording::recover_on_start(&mut state);
        }
        if let Some(name) = &args.theme {
            theme::set_theme(ThemeName::parse(name));
        }
        if let Some(k) = &args.knobs {
            qrs_ui::knobs::set_style(qrs_ui::knobs::KnobStyle::parse(k));
        }
        QuickRs {
            hwnd,
            dlg_gave_up: false,
            open_paused: false,
            open_review: false,
            styled: None,
            screen: if args.gallery {
                Screen::Gallery
            } else {
                Screen::Pages
            },
            gallery: gallery::Gallery::default(),
            shots,
            driver: crate::drive::Driver::new(),
            state,
            lib: LibView::default(),
            home: HomeView::default(),
            loading: None,
            player: None,
            settings: None,
            room: None,
            backdrop: Backdrop::default(),
            dlg_backdrop: Backdrop::default(),
            devstatus: DevStatus::default(),
            board_input: None,
            notewidget: NoteWidget::default(),
            shot_applied: None,
            shot_pressed: false,
            shot_saved: Vec::new(),
            timed: (false, false),
            play: args.play.clone(),
            play_pending: false,
            devswap: (args.screenshots.is_none() && !args.gallery)
                .then(crate::devswap::DevSwap::this)
                .flatten(),
            swap_said: false,
            swap_closing: None,
            resume_song: args.resumed.as_ref().and_then(|r| r.song.clone()),
            resume_at: None,
            lick: None,
            lesson: None,
            setup: None,
            setup_checked: false,
        }
    }

    fn update_scale(&self, ctx: &egui::Context) {
        // (the window's pixels as `raw_input_hook` reads them, where it can: it sets the zoom, D70)
        let own = self
            .hwnd
            .and_then(client_pixels)
            .filter(|p| p.x >= 1.0 && p.y >= 1.0);
        let physical = own.unwrap_or_else(|| ctx.content_rect().size() * ctx.pixels_per_point());
        // text sizes round as the Python app's do, from its scale of the window's whole-pixel size
        let (w, h) = (physical.x.round() as f64, physical.y.round() as f64);
        qrs_ui::units::set_scale((w / 1400.0).min(h / 820.0).clamp(0.7, 2.2));
        let native = ctx.native_pixels_per_point().unwrap_or(1.0);
        let zoom = scale_for(physical) / native;
        if (ctx.zoom_factor() - zoom).abs() > 1e-3 {
            ctx.set_zoom_factor(zoom);
            qrs_ui::material::warm(scale_for(physical));
        }
    }

    fn requests(&mut self, ctx: &egui::Context) {
        let mut reqs: Vec<Req> = std::mem::take(&mut self.lib.reqs);
        reqs.append(&mut self.home.reqs);
        for r in reqs {
            match r {
                Req::Fullscreen => toggle_fullscreen(ctx),
                Req::Play { g, part } => self.open_song(g, &part),
                Req::Open { g, part } => {
                    self.open_song(g, &part);
                    self.open_paused = true;
                }
                Req::Review { g, part } => {
                    self.open_song(g, &part);
                    self.open_paused = true;
                    self.open_review = true;
                }
                Req::Lick {
                    g,
                    part,
                    start,
                    end,
                    name,
                } => {
                    self.open_song(g, &part);
                    self.lick = Some((start, end, name));
                }
                Req::Lesson {
                    g,
                    part,
                    lesson,
                    riff,
                } => {
                    if let Some(l) = self.state.lesson(&lesson) {
                        self.open_song(g, &part);
                        self.lesson = Some((l, riff));
                    }
                }
                Req::Settings(section) => self.open_settings(section),
                Req::Setup => self.open_setup(),
                Req::Room => self.open_room(),
            }
        }
    }

    /// What the downloads card asked for: Open in the amp room shows the room's catalogue on the new capture.
    fn download_act(&mut self, act: crate::downloads::Act) {
        let crate::downloads::Act::OpenRoom(it) = act;
        if self.settings.is_some() {
            self.close_settings();
        }
        if self.room.is_none() {
            if self.player.is_some() {
                self.open_room_song();
            } else {
                self.open_room();
            }
        }
        if let Some(r) = &mut self.room {
            r.show_capture(&mut self.state, &it.name);
        }
    }

    /// The amp room (from Home's AMP ROOM button or A).
    fn open_room(&mut self) {
        if self.room.is_none() && self.player.is_none() && self.loading.is_none() {
            self.state.previewer.stop();
            self.room = Some(crate::room::RoomView::open(&mut self.state));
        }
    }

    /// The amp room over the player, in the song's context (its Pedalboard tile's expand key, D102): the song
    /// pauses; leaving goes back to it with the tone the room left playing.
    fn open_room_song(&mut self) {
        if self.room.is_some() || self.loading.is_some() {
            return;
        }
        let Some(p) = &mut self.player else { return };
        p.hold();
        let song = p.rack.song.clone();
        let (pick, tone) = (p.rack.board.pick, p.rack.board.song_tone);
        self.room = Some(crate::room::RoomView::open_song(
            &mut self.state,
            song,
            pick,
            tone,
        ));
    }

    fn close_room(&mut self) {
        crate::recording::leaving(&mut self.state, crate::recording::Page::Room);
        if let Some(mut r) = self.room.take() {
            r.leave(&mut self.state);
            if r.song
                && let Some(p) = &mut self.player
            {
                p.rack.reload_song(&self.state, r.rack.board.pick);
                return;
            }
        }
        self.lib.view = View::Home;
    }

    /// A frame of the room: its keys, then it; Esc or Home leaves it.
    fn room_frame(&mut self, ui: &mut egui::Ui, rect: egui::Rect) {
        let ctx = ui.ctx().clone();
        let events = ctx.input(|i| i.events.clone());
        if let Some(r) = &mut self.room {
            for ev in &events {
                r.handle(&mut self.state, ev);
            }
            r.ui(ui, &mut self.state, rect);
            let want = r.want_settings.take();
            if let Some(sec) = want {
                self.open_settings(sec);
                return;
            }
            let Some(r) = &mut self.room else { return };
            if r.closed {
                self.close_room();
                ctx.request_repaint();
            }
        }
    }

    /// A song file dropped on the window: its part picker (like the Python app's drag and drop).
    fn open_dropped(&mut self, path: std::path::PathBuf) {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        if self.player.is_some() {
            self.close_player();
        }
        if self.room.is_some() {
            self.close_room();
        }
        if self.settings.is_some() {
            self.close_settings();
        }
        // a folder (not a folder-form song): its songs join the library for good
        if path.is_dir() && !path.join("manifest.yaml").is_file() {
            match self.state.add_folder(&path) {
                Ok(msg) => self.state.say(msg),
                Err(e) => self.state.say_error(format!("{name}: {e}")),
            }
            return;
        }
        match self.state.add_dropped(&path) {
            Ok(g) => {
                if self.lib.view == View::Home {
                    self.lib.view = View::All;
                }
                self.lib.refresh(&self.state);
                self.lib.open_picker(&self.state, g);
            }
            Err(e) => self.state.say_error(format!("{name}: {e}")),
        }
    }

    pub fn open_song(&mut self, g: usize, part: &str) {
        self.state.previewer.stop();
        let e = self.state.song(g).clone();
        let rate = self.state.audio.as_ref().map(|a| a.engine.rate());
        self.lick = None;
        self.lesson = None;
        self.open_paused = false;
        self.open_review = false;
        self.loading = Some(Loading::start(g, &e, part, &self.state.keys, rate));
    }

    fn close_player(&mut self) {
        crate::recording::leaving(&mut self.state, crate::recording::Page::Player);
        if let Some(mut p) = self.player.take() {
            p.board_leave(&mut self.state);
            let riff = p.coach.as_ref().map(|c| (c.lesson.id.clone(), c.riff.id.clone()));
            p.coach_close(&mut self.state);
            p.close();
            if let Some((lesson, riff)) = riff {
                // a lesson's riff: back on the Lessons page, with that riff selected
                self.lib.select_riff(&mut self.state, &lesson, &riff);
            } else {
                // back on the library with the song you played selected (its row now: the library may have been
                // read again while it played)
                self.lib.refresh(&self.state);
                let g = self.state.row_of(&p.entry).unwrap_or(p.g);
                if let Some(i) = self.lib.items.iter().position(|&x| x == g) {
                    self.lib.sel = i;
                    self.lib.top = i.saturating_sub(2);
                }
            }
        }
        self.state.save_config();
        self.state.version += 1; // the scores may have changed
    }

    /// Open a lesson's riff: its song on the riff's part, then the coach (as the Lessons page does).
    fn open_riff(&mut self, lesson: &str, riff: &str) {
        let Some(l) = self.state.lesson(lesson) else {
            return;
        };
        let Some(r) = l.riff(riff).cloned() else {
            return;
        };
        self.lib.select_riff(&mut self.state, lesson, riff);
        let Some(g) = self.state.riff_row(&r) else {
            self.state
                .say(format!("{} by {} isn't in your library", r.title, r.artist));
            return;
        };
        let e = self.state.song(g);
        if e.needs_keys {
            self.state.say(format!(
                "{} needs RS's keys: add them in Settings > Directories",
                r.title
            ));
            return;
        }
        let part = r
            .part_of(&e.arrangements)
            .map(|i| &e.arrangements[i])
            .or_else(|| crate::library::last_part(&self.state, e))
            .map(|a| a.key.clone());
        let Some(part) = part else { return };
        self.open_song(g, &part);
        self.lesson = Some((l, r.id));
    }

    fn player_requests(&mut self) {
        let reqs = self
            .player
            .as_mut()
            .map(|p| std::mem::take(&mut p.reqs))
            .unwrap_or_default();
        for r in reqs {
            match r {
                PlayerReq::Library | PlayerReq::Lessons => self.close_player(),
                PlayerReq::Settings(section) => self.open_settings(section),
                PlayerReq::Room => self.open_room_song(),
                PlayerReq::Riff { lesson, riff } => {
                    self.close_player();
                    self.open_riff(&lesson, &riff);
                }
                PlayerReq::Song { id } => {
                    // (a song from the pedalboard editor's "in your library")
                    let row = self
                        .state
                        .songs
                        .iter()
                        .position(|e| e.id() == id)
                        .and_then(|i| self.state.groups.iter().position(|g| g.members.contains(&i)));
                    if let Some(g) = row {
                        let e = self.state.song(g).clone();
                        if let Some(part) = crate::library::last_part(&self.state, &e).map(|a| a.key.clone())
                        {
                            self.close_player();
                            self.open_song(g, &part);
                        }
                    }
                }
            }
        }
    }

    pub fn open_settings(&mut self, section: &str) {
        if self.loading.is_some() {
            return;
        }
        if let Some(p) = &mut self.player {
            p.hold(); // (a drag's button-up would go to the panel)
        }
        self.backdrop.clear();
        self.settings = Some(SettingsView::new(section));
    }

    fn close_settings(&mut self) {
        self.settings = None;
        self.backdrop.clear();
        self.state.save_config();
        if let Some(p) = &mut self.player {
            p.reload_settings(&self.state);
        }
    }

    /// The settings panel over the page it was opened from: the page is drawn alone until a capture of it is in
    /// (the capture is of the frame after the request, so two frames), then shown blurred under the panel (the song
    /// is paused, so nothing on it moves; the player keeps updating).
    fn settings_frame(&mut self, ui: &mut egui::Ui, rect: egui::Rect) {
        let ctx = ui.ctx().clone();
        let events = ctx.input(|i| i.events.clone());
        if let Some(s) = &mut self.settings {
            for ev in &events {
                if matches!(
                    ev,
                    Event::Key {
                        key: Key::F2 | Key::F11,
                        ..
                    }
                ) {
                    continue;
                }
                s.handle(&mut self.state, ev);
            }
        }
        let draw_panel = if self.backdrop.ready(&ctx) {
            self.backdrop.draw(ui.painter(), rect);
            if let Some(p) = &mut self.player {
                p.update(&mut self.state);
            }
            true
        } else {
            if let Some(p) = &mut self.player {
                p.ui_behind(ui, &mut self.state, rect);
            } else {
                self.pages(ui, rect, false);
            }
            if !self.backdrop.capturing() {
                self.backdrop.capture(&ctx); // the next frame, the page alone like this one, is captured
            }
            false
        };
        if let Some(s) = &mut self.settings {
            s.ui(ui, &mut self.state, self.player.as_mut(), rect, draw_panel);
            if s.closed {
                let setup = s.first_setup;
                let at = s.first_setup_at.take();
                let room = s.open_room;
                self.close_settings();
                if room {
                    // (Settings › Tone's "Edit in the amp room": over the song when one is open)
                    if self.player.is_some() {
                        self.open_room_song();
                    } else {
                        self.open_room();
                    }
                }
                if setup {
                    self.open_setup();
                    if let Some(page) = at
                        && self.setup.is_some()
                    {
                        self.setup = Some(crate::settings::FirstSetup::at(&page));
                    }
                }
            }
        }
    }

    /// The first-time setup opens by itself on a first run, or for a config with nothing of a user's in it (not
    /// in screenshots, once a start).
    fn check_setup(&mut self) {
        if self.setup_checked || self.shots.is_some() {
            return;
        }
        self.setup_checked = true;
        let st = &mut self.state;
        let keys = matches!(st.key_state, crate::keyfile::KeyState::Found { .. });
        let due = crate::setup::due(&st.config, st.first_run, keys);
        // an update: Home asks whether to run the setup again (P37.15); this version is the last run from now on
        st.whats_new = crate::whatsnew::on_start(
            &mut st.config,
            st.first_run,
            due,
            &crate::update::Version::current(),
        );
        st.save_config();
        if due {
            self.open_setup();
        }
    }

    /// The first-time setup over the page that's up (a first run, Settings' button, Home's Getting started).
    pub fn open_setup(&mut self) {
        if self.loading.is_some() {
            return;
        }
        if let Some(p) = &mut self.player {
            p.hold();
        }
        self.settings = None;
        self.backdrop.clear();
        self.setup = Some(crate::settings::FirstSetup::new());
    }

    fn close_setup(&mut self) {
        let room = self.setup.take().is_some_and(|s| s.room);
        self.backdrop.clear();
        self.state.save_config();
        if let Some(p) = &mut self.player {
            p.reload_settings(&self.state);
        }
        if room {
            if self.player.is_some() {
                self.close_player();
            }
            self.open_room();
        }
    }

    /// The setup over the page it was opened on, as the settings panel is (the page captured, then blurred under it).
    fn setup_frame(&mut self, ui: &mut egui::Ui, rect: egui::Rect) {
        let ctx = ui.ctx().clone();
        let events = ctx.input(|i| i.events.clone());
        if let Some(s) = &mut self.setup {
            for ev in &events {
                if matches!(
                    ev,
                    Event::Key {
                        key: Key::F2 | Key::F11,
                        ..
                    }
                ) {
                    continue;
                }
                s.handle(&mut self.state, ev);
            }
        }
        let draw_panel = if self.backdrop.ready(&ctx) {
            self.backdrop.draw(ui.painter(), rect);
            if let Some(p) = &mut self.player {
                p.update(&mut self.state);
            }
            true
        } else {
            if let Some(p) = &mut self.player {
                p.ui_behind(ui, &mut self.state, rect);
            } else {
                self.pages(ui, rect, false);
            }
            if !self.backdrop.capturing() {
                self.backdrop.capture(&ctx);
            }
            false
        };
        if let Some(s) = &mut self.setup {
            s.ui(ui, &mut self.state, self.player.as_mut(), rect, draw_panel);
            if s.closed {
                self.close_setup();
            }
        }
    }

    fn pages(&mut self, ui: &mut egui::Ui, area: egui::Rect, input: bool) {
        let ctx = ui.ctx().clone();
        let events = if input {
            ctx.input(|i| i.events.clone())
        } else {
            Vec::new()
        };
        for ev in &events {
            let at_home = self.lib.view == View::Home
                && self.lib.picker.is_none()
                && self.lib.menu.is_none()
                && self.lib.text_box.is_none();
            if at_home {
                self.home.handle(&mut self.lib, &mut self.state, ev);
            } else {
                self.lib.handle(&mut self.state, area, ev);
            }
        }
        // Home with a song's picker (or a name box) open over it (owner request, 2026-11-02: a click on a song
        // there opens its picker where you are)
        let home = self.lib.view == View::Home;
        if home && self.lib.picker.is_none() && self.lib.text_box.is_none() {
            self.home.ui(ui, &mut self.lib, &mut self.state, area);
            if self.lib.view != View::Home || self.lib.picker.is_some() || self.lib.text_box.is_some() {
                ctx.request_repaint(); // the sidebar went to the library or a card opened a picker: draw it next frame
            }
        } else {
            // glass dialogs: the page drawn alone until a capture of it is in (two frames), then its frosted copy
            // under them (as the settings panel's backdrop). A capture that hasn't come in 250 ms isn't waited for:
            // the dialog goes over the page as it is (it once never came, and the page sat there without its picker)
            let frost = input && self.lib.has_dialog() && theme::is_dark() && !theme::classic();
            self.dlg_gave_up |= self
                .dlg_backdrop
                .waiting()
                .is_some_and(|w| w > std::time::Duration::from_millis(250));
            let waited = frost && self.dlg_gave_up;
            if !frost || waited {
                if !frost {
                    self.dlg_backdrop.clear();
                    self.dlg_gave_up = false;
                }
                if home {
                    self.home.ui(ui, &mut self.lib, &mut self.state, area);
                    self.lib.ui_part(ui, &mut self.state, area, Part::Dialogs);
                } else {
                    self.lib.ui(ui, &mut self.state, area);
                }
                if waited && self.dlg_backdrop.ready(&ctx) {
                    self.dlg_gave_up = false;
                    ctx.request_repaint(); // (it came after all: frosted from the next frame)
                }
            } else if self.dlg_backdrop.ready(&ctx) {
                self.dlg_backdrop.draw_page(ui.painter(), ctx.content_rect());
                self.lib.ui_part(ui, &mut self.state, area, Part::Dialogs);
            } else {
                if home {
                    self.home.ui(ui, &mut self.lib, &mut self.state, area);
                } else {
                    self.lib.ui_part(ui, &mut self.state, area, Part::Page);
                }
                if !self.dlg_backdrop.capturing() {
                    self.dlg_backdrop.capture(&ctx);
                }
                ctx.request_repaint_after(std::time::Duration::from_millis(30)); // (until it's in, or given up)
            }
        }
        self.requests(&ctx);
    }

    /// The song the Python app's screenshots open: Here Without You (else the first playable one).
    /// QUICKRS_SHOT_SONG picks another: the first song whose title contains it (any case).
    fn shot_song(&self) -> Option<usize> {
        let n = self.state.groups.len();
        let want = std::env::var("QUICKRS_SHOT_SONG").ok().map(|s| s.to_lowercase());
        (0..n)
            .find(|&g| {
                let title = self.state.song(g).display_title();
                match &want {
                    Some(w) => title.to_lowercase().contains(w.as_str()),
                    None => title.eq_ignore_ascii_case("Here Without You"),
                }
            })
            .or_else(|| (0..n).find(|&g| self.state.song(g).playable()))
    }

    fn apply_shot(&mut self) {
        let Some(sh) = &self.shots else { return };
        let Some(i) = sh.index() else {
            // the shots are done: settings they changed go back before the config is saved
            if self.room.is_some() {
                self.close_room();
            }
            for (k, v) in std::mem::take(&mut self.shot_saved) {
                match v {
                    Some(v) => self.state.config.set(k, v),
                    None => {
                        self.state.config.remove(k);
                    }
                }
            }
            return;
        };
        if self.shot_applied == Some(i) {
            return;
        }
        let Some((page, shot_theme)) = sh.current().map(|s| (s.page, s.theme)) else {
            return;
        };
        // the library first (the first-time setup's shots: read, songs or not)
        let setup_shot = matches!(page, shots::Page::Setup(_));
        if page != shots::Page::Gallery
            && (self.state.scanning() || (self.state.groups.is_empty() && !setup_shot))
        {
            return;
        }
        self.shot_applied = Some(i);
        if let Some(name) = self
            .shots
            .as_ref()
            .and_then(|s| s.current())
            .map(|s| s.name.clone())
        {
            crate::downloads::stage_shot(&name);
        }
        theme::set_theme(shot_theme);
        self.setup = match page {
            shots::Page::Setup(what) => Some(crate::settings::FirstSetup::staged(what)),
            _ => None,
        };
        self.screen = if page == shots::Page::Gallery {
            Screen::Gallery
        } else {
            Screen::Pages
        };
        if !matches!(page, shots::Page::Settings(_)) {
            self.settings = None;
            self.backdrop.clear();
        }
        if page != shots::Page::DeviceOffer {
            self.devstatus = DevStatus::default(); // (a staged offer stays on its own shot)
        }
        self.state.rec.clear_shot(); // (a staged recording is its shot's only)
        self.state.whats_new = page == shots::Page::HomeNew; // (the card after an update: its own shot only)
        // settings the last shot changed go back; this shot's go in
        for (k, v) in std::mem::take(&mut self.shot_saved) {
            match v {
                Some(v) => self.state.config.set(k, v),
                None => {
                    self.state.config.remove(k);
                }
            }
        }
        let mg_tiles = serde_json::json!([
            ["mg300", true],
            ["note", true],
            ["score", true],
            ["tuner", true],
            ["lyrics", false]
        ]);
        let board_tiles = serde_json::json!([
            ["board", true],
            ["note", true],
            ["score", true],
            ["tuner", false],
            ["lyrics", false]
        ]);
        let staged: Vec<(&'static str, serde_json::Value)> = match page {
            shots::Page::PlayerBoard
            | shots::Page::BoardView
            | shots::Page::PlayerAt("board_add")
            | shots::Page::PlayerAt("board_sounds") => {
                vec![("tiles", board_tiles), ("tone_v2", "board".into())]
            }
            shots::Page::PlayerMg300
            | shots::Page::Mg300View
            | shots::Page::PlayerAt("tiles_scrolled")
            | shots::Page::PlayerAt("tile_hover") => vec![("tiles", mg_tiles)],
            shots::Page::PlayerControls => vec![
                ("controls_tab", "practise".into()),
                ("tiles", serde_json::json!([["board", false]])),
            ],
            shots::Page::PlayerAt("controls_sound") => vec![
                ("controls_tab", "sound".into()),
                ("tiles", serde_json::json!([["board", false]])),
            ],
            shots::Page::PlayerAt("controls_view") => vec![
                ("controls_tab", "view".into()),
                ("tiles", serde_json::json!([["board", false]])),
            ],
            shots::Page::PlayerAt("neck") => vec![("note_view", "guitar".into())],
            shots::Page::PlayerAt(w)
                if w == "tuner_lyrics" || w == "tuner_edit" || w.starts_with("tuner_face_") =>
            {
                vec![(
                    "tiles",
                    serde_json::json!([
                        ["tuner", true],
                        ["score", true],
                        ["lyrics", true],
                        ["note", false]
                    ]),
                )]
            }

            // (the Python app's player shots: its tiles, the pedalboard's off)
            p if p.is_player() => vec![("tiles", serde_json::json!([["board", false]]))],
            _ => Vec::new(),
        };
        for (k, v) in staged {
            self.shot_saved.push((k, self.state.config.get(k).cloned()));
            self.state.config.set(k, v);
        }
        if page == shots::Page::PlayerAt("board_sounds") {
            // your sounds on the tile: the room's (made up when there are none); your board and the room's state
            // go back when the shots move on
            for k in ["tone_board", "room", "instrument", "input_cal"] {
                if !self.shot_saved.iter().any(|(s, _)| *s == k) {
                    self.shot_saved.push((k, self.state.config.get(k).cloned()));
                }
            }
            crate::room::stage_data(&mut self.state);
        }
        if page != shots::Page::Lib("loading") && self.loading.as_ref().is_some_and(|l| l.is_stub()) {
            self.loading = None;
        }
        if page.is_player() {
            if let Some(p) = &mut self.player {
                p.prepare_shot(&mut self.state, Some(page));
                if let shots::Page::Settings(sec) = page {
                    match &mut self.settings {
                        Some(s) => s.show(sec),
                        None => self.open_settings(sec),
                    }
                }
            } else if self.loading.is_none()
                && let Some(g) = self.shot_song()
            {
                // the song the Python app's screenshots use, in its first part (as they open it);
                // QUICKRS_SHOT_PART picks another by its key ("rhythm", "lead2", "combo": the highway compare harness)
                let arrs = &self.state.song(g).arrangements;
                let want = std::env::var("QUICKRS_SHOT_PART")
                    .ok()
                    .map(|s| s.trim().to_lowercase());
                let part = want
                    .and_then(|w| arrs.iter().find(|a| a.key == w))
                    .or_else(|| arrs.first())
                    .map(|a| a.key.clone())
                    .unwrap_or_default();
                self.open_song(g, &part);
            }
            // the room in the song's context (P37.1)
            let room_page = matches!(
                page,
                shots::Page::PlayerAt("room_song") | shots::Page::PlayerAt("room_explain")
            );
            if room_page && self.player.is_some() {
                if self.room.as_ref().is_some_and(|r| !r.song) {
                    self.close_room();
                }
                if self.room.is_none() {
                    self.open_room_song();
                }
                if let Some(r) = &mut self.room {
                    r.shot = true;
                    r.listen.stage(-16.0, 45.04, false);
                    r.rig_ui.explain = (page == shots::Page::PlayerAt("room_explain")).then_some(0);
                }
            } else if self.room.as_ref().is_some_and(|r| r.song) {
                self.close_room();
            }
            return;
        }
        if self.player.is_some() {
            self.close_player();
        }
        self.lib.picker = None;
        self.lib.menu = None;
        self.lib.text_box = None;
        self.lib.query.clear();
        // the AMP ROOM button staged on its shots only; the room on its own
        self.lib.amp.listen = Default::default();
        self.lib.amp.shot_hover = false;
        if !matches!(page, shots::Page::Room(_)) && self.room.is_some() {
            self.close_room();
        }
        match page {
            shots::Page::Gallery => {}
            shots::Page::Home => self.lib.view = View::Home,
            shots::Page::HomeNew => self.lib.view = View::Home,
            shots::Page::HomeAmp(what) => {
                self.lib.view = View::Home;
                // E2 three cents sharp at a good level (or clipping)
                self.lib.amp.listen.stage(-19.0, 40.03, what == "hot");
                self.lib.amp.shot_hover = what == "hover";
            }
            shots::Page::Room(what) => {
                self.lib.view = View::Home;
                self.stage_room(what);
            }
            shots::Page::Setup(_) => self.lib.view = View::Home,
            shots::Page::NoteWidget | shots::Page::NoteWidgetChords => self.lib.view = View::Home,
            shots::Page::DeviceOffer => {
                self.lib.view = View::Home;
                self.devstatus.stage("NUX Audio", "audio interface (ASIO driver)");
            }
            shots::Page::Library => {
                self.lib.view = View::All;
                self.lib.sel = 0;
                self.lib.top = 0;
            }
            shots::Page::LibrarySelected => {
                self.lib.view = View::All;
                self.lib.sel = 3;
                self.lib.top = 0;
            }
            shots::Page::Picker => {
                // as the Python tool: "here without" searched, the first song's part picker
                self.lib.view = View::All;
                self.lib.query = "here without".into();
                self.lib.sel = 0;
                self.lib.top = 0;
                self.lib.refresh(&self.state);
                if let Some(g) = self.lib.selected() {
                    self.lib.open_picker(&self.state, g);
                }
            }
            shots::Page::Search => {
                self.lib.view = View::All;
                self.lib.query = "the".into();
                self.lib.sel = 0;
                // (from the top: a player shot before this one leaves the list scrolled to its song's row, past
                // the end of the search's few rows, and the list drew empty)
                self.lib.top = 0;
                self.lib.refresh(&self.state);
            }
            shots::Page::Lib("licks") => {
                self.stage_licks();
                self.lib.view = View::Licks;
                self.lib.sel = 0;
                self.lib.top = 0;
                self.lib.refresh(&self.state);
            }
            shots::Page::Lib("lessons") => {
                self.stage_lessons();
                self.lib.go(&mut self.state, View::Lessons);
                self.lib.refresh(&self.state);
            }
            shots::Page::Lib(what @ ("best" | "playlist")) => {
                // the rank badges and the Random key (P37.10): all songs by best score, or the first playlist (one
                // made up of the first few songs when there's none: the shot showed All songs, P37.4)
                if what == "playlist" && self.state.userdata.playlists.is_empty() {
                    let st = &self.state;
                    let ids: Vec<String> = (0..st.groups.len())
                        .filter(|&g| st.listed(g) && st.song(g).playable())
                        .take(8)
                        .map(|g| st.song(g).id())
                        .collect();
                    self.state.userdata.playlists.push(("Warm-ups".into(), ids));
                }
                self.lib.view = match self.state.userdata.playlists.first() {
                    Some((name, _)) if what == "playlist" => View::Playlist(name.clone()),
                    _ => View::All,
                };
                self.lib.sort = if what == "best" { 4 } else { 0 };
                self.lib.sel = 0;
                self.lib.top = 0;
                self.lib.refresh(&self.state);
            }
            shots::Page::Lib(what @ ("difficulty" | "easy")) => {
                // P37.6b (D137): all songs by difficulty, easiest first; the sidebar's Easy songs
                self.lib.view = if what == "easy" { View::Level(0) } else { View::All };
                self.lib.query.clear();
                self.lib.sort = crate::library::sort_index("difficulty");
                self.lib.sel = 0;
                self.lib.top = 0;
                self.lib.refresh(&self.state);
            }
            shots::Page::Lib(what) => {
                self.lib.sort = 0;
                self.lib.view = if what == "fav" { View::Fav } else { View::All };
                if what != "fav" {
                    self.lib.query = "here without".into(); // (the Python tool's search is still in)
                }
                self.lib.sel = 0;
                self.lib.top = 0;
                self.lib.refresh(&self.state);
                match what {
                    "menu" => {
                        // the playlist menu over the picker, for the picker's song
                        if let Some(g) = self.lib.selected() {
                            self.lib.open_picker(&self.state, g);
                            self.lib.menu = Some(g);
                        }
                    }
                    "newpl" => {
                        self.lib.text_box = Some(crate::library::TextBox {
                            title: "New playlist name".into(),
                            text: "Sunday practice".into(),
                            verb: "Create".into(),
                            skip: None,
                            then: crate::library::TextThen::NewPlaylist { add: None },
                        })
                    }
                    "loading" => {
                        if let Some(g) = self.shot_song() {
                            let title = self.state.song(g).display_title();
                            self.loading = Some(Loading::stub(&title));
                        }
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }
}

impl QuickRs {
    /// Screenshot mode: the amp room in a state, with sounds saved and songs' tones to find (made up when there are
    /// none).
    fn stage_room(&mut self, what: &'static str) {
        // (your board and the room's state go back when the shots move on)
        for k in ["tone_board", "room"] {
            if !self.shot_saved.iter().any(|(s, _)| *s == k) {
                self.shot_saved.push((k, self.state.config.get(k).cloned()));
            }
        }
        if self.room.is_none() {
            crate::room::stage_data(&mut self.state);
            self.room = Some(crate::room::RoomView::open(&mut self.state));
        }
        if let Some(r) = &mut self.room {
            r.stage_shot(&mut self.state, what);
        }
    }

    /// Screenshot mode: the first lesson's first item done and its second one under way (a theory item at its third
    /// page), when no lesson has been started.
    fn stage_lessons(&mut self) {
        let st = &mut self.state;
        if !st.userdata.lessons.is_empty() {
            return;
        }
        let Some(l) = st.lessons.first().cloned() else {
            return;
        };
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs_f64())
            .unwrap_or(0.0);
        if let Some(r) = l.riffs.first() {
            let mut p = qrs_library::RiffProgress {
                step: if r.kind == qrs_library::lessons::Kind::Ear {
                    "check"
                } else {
                    "done"
                }
                .into(),
                done: true,
                listens: 5,
                found: r.phrase as u32,
                wrong: 14,
                hints: 1,
                seconds: 1260.0,
                started: now - 86400.0,
                finished: now - 82000.0,
                ..Default::default()
            };
            p.record(0.7, 0.96);
            p.record(0.85, 0.92);
            p.record(1.0, 0.94);
            st.userdata.set_riff_progress(&l.id, &r.id, &p);
        }
        if let Some(r) = l.riffs.get(1) {
            let p = qrs_library::RiffProgress {
                step: match r.kind {
                    qrs_library::lessons::Kind::Theory => "p2",
                    qrs_library::lessons::Kind::Learn => "practise",
                    _ => "notes",
                }
                .into(),
                listens: 5,
                found: 4,
                wrong: 6,
                seconds: 420.0,
                started: now - 3600.0,
                ..Default::default()
            };
            st.userdata.set_riff_progress(&l.id, &r.id, &p);
        }
    }

    /// Screenshot mode: three saved licks (from the first playable songs) when the user data has none.
    fn stage_licks(&mut self) {
        if !self.state.userdata.licks.is_empty() {
            return;
        }
        let st = &mut self.state;
        let rows: Vec<usize> = (0..st.groups.len())
            .filter(|&g| st.listed(g) && st.song(g).playable())
            .take(3)
            .collect();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs_f64())
            .unwrap_or(0.0);
        let staged = [
            ("Chorus", (29, 31), 61.2, 72.4),
            ("Solo", (45, 48), 118.0, 131.5),
            ("Intro", (1, 4), 0.8, 9.6),
        ];
        for (n, &g) in rows.iter().enumerate() {
            let (sec, bars, a, b) = staged[n];
            let e = st.song(g).clone();
            let Some(part) = e.arrangements.first() else {
                continue;
            };
            let sections = vec![sec.to_string()];
            st.userdata.add_lick(qrs_library::Lick {
                song: e.id(),
                canonical: st.groups[g].canonical.clone(),
                part: part.name.clone(),
                part_key: part.key.clone(),
                start: a,
                end: b,
                title: e.display_title(),
                artist: e.artist.clone(),
                tuning: part.tuning.clone(),
                name: qrs_library::Lick::auto_name(&sections, Some(bars), a),
                sections,
                bars: Some(bars),
                saved: now - 3600.0 * 26.0 * (2 - n) as f64,
                other: Default::default(),
            });
        }
    }
}

pub fn toggle_fullscreen(ctx: &egui::Context) {
    let fs = ctx.input(|i| i.viewport().fullscreen.unwrap_or(false));
    ctx.send_viewport_cmd(ViewportCommand::Fullscreen(!fs));
}

impl eframe::App for QuickRs {
    /// Screenshot mode: the real mouse and keyboard don't reach the app (a shot shows only what it stages, like the
    /// Python tool's off-screen window); the shots of something under the pointer get a pointer there.
    fn raw_input_hook(&mut self, ctx: &egui::Context, raw_input: &mut egui::RawInput) {
        if let Some(d) = &mut self.driver {
            d.input(ctx, raw_input);
        }
        // the window's scale set before the frame starts, so a resize lays the page out once, at the new scale (it
        // took two frames, the first redrawing the panels at the old one: D70). The size is the window's own, in
        // pixels: egui's rect here is in points of the scale being replaced.
        // (not before the first frame: egui has no screen for it yet, and changing the scale there left it at its
        // 10000-point stand-in; `update_scale` sets the first frame's, from the same pixels)
        if let Some(physical) = self.hwnd.filter(|_| self.timed.0).and_then(client_pixels) {
            let native = raw_input
                .viewports
                .get(&raw_input.viewport_id)
                .and_then(|v| v.native_pixels_per_point)
                .or(ctx.native_pixels_per_point())
                .unwrap_or(1.0);
            let zoom = scale_for(physical) / native;
            if physical.x >= 1.0 && physical.y >= 1.0 && (ctx.zoom_factor() - zoom).abs() > 1e-3 {
                ctx.set_zoom_factor(zoom);
                qrs_ui::material::warm(scale_for(physical));
                // and the frame's screen in the new scale's points: egui's is in the old one's, and laid out on it
                // the frame came out 12500 pixels wide (the first frame of every start, and of each resize step)
                raw_input.screen_rect = Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    physical / (native * zoom),
                ));
            }
        }
        let Some((page, pointer)) = self
            .shots
            .as_ref()
            .and_then(|s| s.current())
            .map(|s| (s.page, s.pointer))
        else {
            return;
        };
        raw_input.events.retain(|e| {
            !matches!(
                e,
                Event::PointerMoved(_)
                    | Event::PointerButton { .. }
                    | Event::PointerGone
                    | Event::MouseWheel { .. }
                    | Event::MouseMoved(_)
                    | Event::Key { .. }
                    | Event::Text(_)
            )
        });
        let s = qrs_ui::units::scale() as f32;
        let at = match pointer {
            Some((x, y, _)) if s > 0.0 => Some(egui::pos2(x / s, y / s)),
            _ => self.player.as_ref().and_then(|p| p.shot_pointer(page)),
        };
        let down = pointer.is_some_and(|p| p.2);
        if self.shot_pressed && !down {
            // let go away from the control (no click)
            let away = egui::pos2(-100.0, -100.0);
            raw_input.events.push(Event::PointerMoved(away));
            raw_input.events.push(Event::PointerButton {
                pos: away,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            });
            self.shot_pressed = false;
        }
        raw_input.events.push(match at {
            Some(pos) => Event::PointerMoved(pos),
            None => Event::PointerGone,
        });
        if down
            && !self.shot_pressed
            && let Some(pos) = at
        {
            raw_input.events.push(Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            });
            self.shot_pressed = true;
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let _timer = FrameTimer::new(&ctx);
        // a board set before the captures library opened gets its NAM blocks once it has (P37.2, AUDIT B11)
        if crate::captures::is_ready()
            && let Some(a) = self.state.audio.as_ref()
        {
            a.engine.refresh_files();
        }
        if !self.timed.0 {
            self.timed.0 = true;
            crate::gpu::drawn();
            let px = ctx.content_rect().size() * ctx.pixels_per_point();
            crate::applog::write(&format!(
                "startup: first frame at {} ms, window {:.0} x {:.0} pixels",
                crate::applog::since_start_ms(),
                px.x,
                px.y
            ));
        }
        if !self.timed.1 && !self.state.scanning() && !self.state.groups.is_empty() {
            self.timed.1 = true;
            crate::applog::write(&format!(
                "startup: library ready at {} ms ({} songs, {} rows)",
                crate::applog::since_start_ms(),
                self.state.songs.len(),
                self.state.groups.len()
            ));
        }
        self.apply_shot();
        self.update_scale(&ctx);
        if self.styled != Some(theme::theme()) {
            if let Some(h) = self.hwnd {
                chrome::style_title_bar(h);
            }
            self.styled = Some(theme::theme());
        }
        if ctx.input(|i| i.key_pressed(Key::F11)) {
            toggle_fullscreen(&ctx);
        }
        // --play: open that song when the library is ready, and start it when it's open
        if let Some(q) = self.play.clone()
            && !self.state.scanning()
            && !self.state.groups.is_empty()
        {
            let ql = q.to_lowercase();
            if let Some(g) = (0..self.state.groups.len())
                .find(|&g| self.state.song(g).display_title().to_lowercase().contains(&ql))
            {
                let part = crate::library::last_part(&self.state, self.state.song(g))
                    .map(|a| a.key.clone())
                    .unwrap_or_default();
                self.open_song(g, &part);
                self.play_pending = true;
            } else {
                self.state.say(format!("No song called {q:?}"));
            }
            self.play = None;
        }
        if self.play_pending
            && let Some(p) = &mut self.player
        {
            self.play_pending = false;
            p.start_now();
        }
        // a new dev build swapped in (D67): the song that was open, at the time it was at
        if let Some(song) = self.resume_song.clone()
            && !self.state.scanning()
            && !self.state.groups.is_empty()
        {
            self.resume_song = None;
            let songs = self.state.songs.clone();
            match (0..self.state.groups.len()).find(|&g| {
                self.state.groups[g]
                    .members
                    .iter()
                    .any(|&i| songs[i].path == song.path)
            }) {
                Some(g) => {
                    self.open_song(g, &song.part);
                    self.resume_at = Some((song.at, song.playing));
                }
                None => self
                    .state
                    .say("New build in: couldn't find the song you had open"),
            }
        }
        if let Some((at, playing)) = self.resume_at
            && let Some(p) = &mut self.player
        {
            self.resume_at = None;
            p.seek(at);
            if playing {
                p.start_now();
            }
        }
        // a song file dropped on the window opens its part picker
        let dropped: Vec<std::path::PathBuf> = ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .map(|f| f.path().to_path_buf())
                .collect()
        });
        // (the first-time setup: a folder dropped joins its list, the way to add one on a desktop with no file
        // picker, P28.3)
        if self.shots.is_none() && self.setup.is_some() {
            for path in dropped.iter().filter(|p| p.is_dir()) {
                match self.state.add_folder(path) {
                    Ok(msg) => self.state.say(msg),
                    Err(e) => self.state.say_error(e),
                }
            }
        } else if self.shots.is_none() && self.loading.is_none() {
            for path in dropped {
                let ext = path
                    .extension()
                    .map(|e| e.to_string_lossy().to_lowercase())
                    .unwrap_or_default();
                // a song (file or folder pack), or a folder of songs
                if matches!(ext.as_str(), "psarc" | "feedpak" | "sloppak") || path.is_dir() {
                    self.open_dropped(path);
                    break;
                }
            }
        }
        // F2: the settings panel, over whatever page is showing (not while a song is loading, nor the first-time
        // setup)
        if ctx.input(|i| i.key_pressed(Key::F2))
            && self.loading.is_none()
            && self.screen == Screen::Pages
            && self.room.is_none()
            && self.setup.is_none()
        {
            if self.settings.is_some() {
                self.close_settings();
            } else {
                self.open_settings("audio");
            }
        }
        self.state.refresh();
        self.check_setup();
        // a new dev build at the exe's path (D67): swap it in, unless a song is playing or a recording running
        if let Some(ds) = &mut self.devswap
            && ds.poll()
        {
            let playing = self.player.as_ref().is_some_and(|p| p.transport.playing());
            if playing || self.state.rec.status().is_some() {
                if !self.swap_said {
                    self.swap_said = true;
                    self.state.say(if playing {
                        "New build ready: it comes in when you pause"
                    } else {
                        "New build ready: it comes in when the recording stops"
                    });
                }
            } else {
                let z = ctx.zoom_factor();
                let (outer, inner, maximized, fullscreen) = ctx.input(|i| {
                    let v = i.viewport();
                    (v.outer_rect, v.inner_rect, v.maximized, v.fullscreen)
                });
                let resume = crate::devswap::Resume {
                    song: self.player.as_ref().map(|p| crate::devswap::ResumeSong {
                        path: p.entry.path.clone(),
                        part: p.part_key.clone(),
                        at: p.transport.position(),
                        playing: false,
                    }),
                    window: outer
                        .zip(inner)
                        .map(|(o, i)| [o.min.x * z, o.min.y * z, i.width() * z, i.height() * z]),
                    maximized: maximized.unwrap_or(false),
                    fullscreen: fullscreen.unwrap_or(false),
                };
                let file = crate::devswap::resume_file(&self.state.data.cache());
                let exe = ds.exe().to_path_buf();
                self.devswap = None; // (once)
                match crate::devswap::write_resume(&file, &resume)
                    .and_then(|()| crate::devswap::relaunch(&exe, &file))
                {
                    Ok(()) => {
                        crate::applog::write("devswap: new build started, closing this one");
                        self.swap_closing = Some(std::time::Instant::now());
                    }
                    Err(e) => self
                        .state
                        .say_error(format!("New build ready, but it couldn't start ({e})")),
                }
            }
        }
        // the new build is starting and waits for this one to go: ask to close every frame (one request was once
        // lost, 2026-11-02: the old window stayed and the new one gave up), and after 3 s save what closing saves
        // and exit
        if let Some(since) = self.swap_closing {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            ctx.request_repaint();
            if since.elapsed() > std::time::Duration::from_secs(3) {
                crate::applog::write("devswap: still open 3 s after closing; saving and exiting");
                eframe::App::on_exit(self);
                std::process::exit(0);
            }
        }
        // an update is in place: start it, and close this one
        if self.state.updater.take_restart() {
            match crate::update::relaunch() {
                Ok(()) => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
                Err(e) => self.state.say_error(format!(
                    "Updated; couldn't start it again ({e}): please open quickRS"
                )),
            }
        }
        // each guitar input with its own tone: the pedalboard on a dry interface, "as is" on an amp modeller
        if let Some(t) = crate::pedalboard::follow_input(&mut self.state, &mut self.board_input)
            && let Some(p) = &mut self.player
        {
            p.say(
                format!("Tone: {}", crate::tones::label(&t)),
                "  \u{b7}  this input's tone",
            );
        }
        // the sound devices: lost, back, switched, the channel found (told where you are)
        for (msg, tail) in self.devstatus.poll(&self.state) {
            if let Some(s) = &mut self.setup {
                s.say(format!("{msg}{tail}"));
            } else if let Some(s) = &mut self.settings {
                s.say(format!("{msg}{tail}"));
            } else if let Some(p) = &mut self.player {
                p.say_for(msg, tail, 4.0);
            } else if self.lib.view == View::Home {
                self.devstatus.notice(msg, tail);
            } else {
                self.state.say(format!("{msg}{tail}"));
            }
        }
        // Home has no toast of its own: the app's messages (the first start's "brought over") show as a notice
        if self.lib.view == View::Home
            && self.player.is_none()
            && self.loading.is_none()
            && let Some((msg, at)) = self.state.toast.take()
        {
            self.devstatus.notice_since(msg, at);
        }
        let rect = ctx.content_rect();
        ui.painter().rect_filled(rect, 0.0, t().base);
        if let Some(l) = &self.loading
            && let Some(res) = l.poll()
        {
            let title = l.title.clone();
            self.loading = None;
            // the library may have been read again while the song loaded: its row now, not the one it was opened from
            match res.and_then(|mut loaded| match self.state.row_of(&loaded.entry) {
                Some(g) => {
                    loaded.g = g;
                    Ok(loaded)
                }
                None => Err("it's no longer in the library".to_string()),
            }) {
                Ok(loaded) => {
                    let mut p = PlayerView::new(&mut self.state, loaded);
                    if std::mem::take(&mut self.open_paused) {
                        p.open_paused();
                    }
                    if std::mem::take(&mut self.open_review) {
                        p.show_review();
                    }
                    if let Some((a, b, name)) = self.lick.take() {
                        p.set_lick_loop(a, b, &name);
                    }
                    if let Some((lesson, riff)) = self.lesson.take() {
                        p.start_lesson(&mut self.state, lesson, &riff);
                    }
                    let shot_page = self.shots.as_ref().and_then(|s| s.current()).map(|s| s.page);
                    if let Some(page) = shot_page {
                        p.prepare_shot(&mut self.state, Some(page));
                    }
                    self.player = Some(p);
                    // (a settings shot taken first: its panel over the song just opened)
                    if let Some(shots::Page::Settings(sec)) = shot_page {
                        self.open_settings(sec);
                    }
                }
                Err(e) => {
                    self.lick = None;
                    self.lesson = None;
                    self.open_review = false;
                    self.state.say_error(format!("Couldn't open {title}: {e}"))
                }
            }
        }
        match self.screen {
            Screen::Gallery => self.gallery.ui(ui, rect),
            Screen::Pages => {
                if let Some(l) = &self.loading {
                    l.draw(ui.painter(), rect);
                } else if self.setup.is_some() {
                    self.setup_frame(ui, rect);
                } else if self.settings.is_some() {
                    self.settings_frame(ui, rect);
                } else if self.room.as_ref().is_some_and(|r| r.song) {
                    // (the room over the song that opened it)
                    self.room_frame(ui, rect);
                } else if let Some(p) = &mut self.player {
                    if let Some(t) = crate::shotclock::shot_now() {
                        p.shot_seq_frame(t); // (a screenshot sequence: the song at this frame's time)
                    }
                    p.ui(ui, &mut self.state, rect);
                    self.player_requests();
                } else if self.room.is_some() {
                    self.room_frame(ui, rect);
                } else {
                    self.pages(ui, rect, true);
                }
                if self.loading.is_none() && self.setup.is_none() {
                    self.devstatus.ui(ui, &mut self.state, rect);
                }
                // TONE3000 downloads (P38.3): a card over every page, a badge over a song and in the room
                if self.loading.is_none() {
                    let room = self.room.is_some();
                    let song = self.player.is_some() && self.settings.is_none() && self.setup.is_none();
                    let compact = (room || song) && self.settings.is_none() && self.setup.is_none();
                    let gap = if song && !room { 150.0 } else { 110.0 };
                    if let Some(act) = crate::downloads::ui(ui, rect, compact, self.setup.is_none(), gap) {
                        self.download_act(act);
                    }
                }
                // a recording that stopped by itself; the "Saved" notice over whatever page is up (P23.9)
                if let Some((a, b)) = crate::recording::poll(&mut self.state) {
                    match &mut self.player {
                        Some(p) => p.say_for(a, b, 4.0),
                        None => self.state.say(format!("{a}{b}")),
                    }
                }
                // (above the player's transport, or the room's strip)
                let gap = if self.player.is_some() { 150.0 } else { 110.0 };
                // (over the song, centred on the tab and no wider: centred on the window it ran over the console)
                let area = match self.player.as_ref().and_then(|p| p.well_rect()) {
                    Some(w) => egui::Rect::from_min_max(
                        egui::pos2(w.min.x, rect.min.y),
                        egui::pos2(w.max.x, rect.max.y),
                    ),
                    None => rect,
                };
                crate::recording::saved_notice(ui, &mut self.state, area, gap);
                // a newer version (P20.1): its notice, not over a song
                if self.loading.is_none()
                    && self.player.is_none()
                    && self.shots.is_none()
                    && self.setup.is_none()
                {
                    crate::update::notice(ui, &mut self.state, rect);
                }
            }
        }
        // the note widget: its own little window (in screenshot mode, drawn on its shot instead); not over the
        // first-time setup but on its own page there, to be seen
        let setup_hides = self
            .setup
            .as_ref()
            .is_some_and(|s| s.page() != crate::setup::Page::Widget);
        if self.shots.is_none() && !setup_hides {
            let song = self
                .player
                .as_ref()
                .map(|p| (&p.chart.arr, p.now(), p.transport.playing()));
            self.notewidget.ui(&ctx, &mut self.state, song);
        } else if let Some(page @ (shots::Page::NoteWidget | shots::Page::NoteWidgetChords)) =
            self.shots.as_ref().and_then(|s| s.current()).map(|s| s.page)
        {
            let at = egui::pos2(rect.max.x - 272.0 - 40.0, rect.min.y + 110.0);
            self.notewidget.preview(
                ui,
                egui::Rect::from_min_size(at, egui::vec2(272.0, 196.0)),
                page == shots::Page::NoteWidgetChords,
            );
        }
        // (the room in a song's context, once the song has loaded: P37.1)
        let cur = self.shots.as_ref().and_then(|s| s.current().map(|s| s.page));
        if let Some(pg @ (shots::Page::PlayerAt("room_song") | shots::Page::PlayerAt("room_explain"))) = cur
            && self.player.is_some()
            && self.loading.is_none()
            && self.room.as_ref().is_none_or(|r| !r.song)
        {
            if self.room.is_some() {
                self.close_room(); // (the room on its own, from the shots before)
            }
            self.open_room_song();
            if let Some(r) = &mut self.room {
                r.shot = true;
                r.listen.stage(-16.0, 45.04, false);
                r.rig_ui.explain = (pg == shots::Page::PlayerAt("room_explain")).then_some(0);
            }
        }
        if let Some(d) = &self.driver {
            d.after(&ctx);
        }
        if let Some(sh) = &mut self.shots {
            // pages wait for the library (and the player for its song)
            let ready = match sh.current().map(|s| s.page) {
                Some(shots::Page::Gallery) => true,
                Some(shots::Page::Setup(_)) => !self.state.scanning() && self.player.is_none(),
                Some(p) if p.is_player() => self.player.is_some() && self.loading.is_none(),
                _ => !self.state.scanning() && !self.state.groups.is_empty() && self.player.is_none(),
            };
            sh.step(&ctx, ready);
        }
    }

    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        let c = t().base;
        [
            c.r() as f32 / 255.0,
            c.g() as f32 / 255.0,
            c.b() as f32 / 255.0,
            1.0,
        ]
    }

    fn on_exit(&mut self) {
        let _ = crate::recording::stop(&mut self.state); // (a recording running: saved)
        if let Some(p) = &mut self.player {
            p.coach_close(&mut self.state); // a lesson's riff: where it got to
        }
        if let Some(mut r) = self.room.take() {
            r.leave(&mut self.state); // the room's rig and scenes kept
        }
        self.state.save_config();
    }
}
