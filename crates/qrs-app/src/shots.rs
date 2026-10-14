//! Screenshot mode (`quickrs --screenshots DIR`): sets the window to an exact physical size, shows each page in
//! each theme, lets it settle, captures the frame (eframe's screenshot command) and saves a PNG, then quits. Used
//! to compare the screens with the Python app's (P5.6, PLAN.md section 3). Run it with QUICKRS_DATA pointing at a
//! scratch folder (a config.json with library_dirs, and a copy of userdata.json) so nothing of yours changes; no
//! sound device is opened. QUICKRS_SHOTS (comma-separated parts of names) takes only some of the shots.

use std::path::PathBuf;

use eframe::egui;
use egui::{Event, UserData, ViewportCommand};
use qrs_ui::theme::ThemeName;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Page {
    Gallery,
    Home,
    Library,
    LibrarySelected,
    Picker,
    Search,
    Player,
    PlayerControls,
    PlayerSelection,
    Results,
    /// The settings panel over the player, on this section.
    Settings(&'static str),
    /// Home with a device notice and the "Use this guitar?" offer.
    DeviceOffer,
    /// The player with the MG-300 tile first (the simulated pedal), and with its big view open.
    PlayerMg300,
    Mg300View,
    /// Home with the note widget drawn on it (its own window otherwise).
    NoteWidget,
    /// The library in a state: "menu" (the playlist menu), "newpl" (the name box), "fav", "loading".
    Lib(&'static str),
    /// The player in a state (see `PlayerView::prepare_shot`).
    PlayerAt(&'static str),
}

impl Page {
    pub fn is_player(self) -> bool {
        matches!(
            self,
            Page::Player
                | Page::PlayerControls
                | Page::PlayerSelection
                | Page::Results
                | Page::Settings(_)
                | Page::PlayerMg300
                | Page::Mg300View
                | Page::PlayerAt(_)
        )
    }
}

pub struct Shot {
    pub name: String,
    pub theme: ThemeName,
    pub page: Page,
    /// The pointer over a control, in the Python shots' pixels at 1440 x 900, and whether it's pressed.
    pub pointer: Option<(f32, f32, bool)>,
}

/// The pointer shots (tools/parity/py_shots.py POINTER_SHOTS): (name, page, (x, y) at 1440 x 900, pressed).
const POINTER_SHOTS: [(&str, Page, (f32, f32), bool); 12] = [
    ("50_home_play_hover", Page::Home, (727.0, 347.0), false),
    ("51_home_play_down", Page::Home, (727.0, 347.0), true),
    ("52_home_cover_hover", Page::Home, (397.0, 560.0), false),
    ("53_side_hover", Page::Home, (150.0, 249.0), false),
    ("54_lib_row_hover", Page::Library, (700.0, 378.0), false),
    ("55_lib_sort_hover", Page::Library, (1340.0, 54.0), false),
    ("56_player_fwd_hover", Page::Player, (196.0, 833.0), false),
    ("57_player_gem_down", Page::Player, (127.0, 833.0), true),
    ("58_player_part_hover", Page::Player, (864.0, 54.0), false),
    ("59_player_controls_hover", Page::Player, (1225.0, 831.0), false),
    (
        "60_settings_nav_hover",
        Page::Settings("audio"),
        (240.0, 272.0),
        false,
    ),
    (
        "61_settings_done_down",
        Page::Settings("audio"),
        (1253.0, 80.0),
        true,
    ),
];

pub struct Shots {
    dir: PathBuf,
    target: (u32, u32),
    plan: Vec<Shot>,
    i: usize,
    frames: u32,
    waiting: bool,
    resize_tries: u32,
}

impl Shots {
    /// `theme`: take every shot in this theme (as the Python tool's `--theme`), without the per-theme extras.
    pub fn new(dir: PathBuf, target: (u32, u32), theme: Option<ThemeName>) -> Shots {
        let mut plan = Vec::new();
        for th in ThemeName::ALL {
            plan.push(Shot {
                name: format!("gallery-{}", th.key()),
                theme: th,
                page: Page::Gallery,
                pointer: None,
            });
        }
        // the Python app's shots (tools/parity/py_shots.py), in its order and with its names, then quickRS's own
        let mid = ThemeName::Midnight;
        let py: [(&str, Page); 36] = [
            ("00_home", Page::Home),
            ("01_library", Page::Library),
            ("02_library_selected", Page::LibrarySelected),
            ("03_picker", Page::Picker),
            ("04_playlist_menu", Page::Lib("menu")),
            ("05_new_playlist", Page::Lib("newpl")),
            ("06_library_favourites", Page::Lib("fav")),
            ("07_loading", Page::Lib("loading")),
            ("08_count_in", Page::PlayerAt("countin")),
            ("10_player", Page::Player),
            ("11_player_guitar_neck", Page::PlayerAt("neck")),
            ("12_tiles_tuner_lyrics", Page::PlayerAt("tuner_lyrics")),
            ("12b_tiles_mg300", Page::PlayerMg300),
            ("12d_tile_hover", Page::PlayerAt("tile_hover")),
            ("12c_tiles_scrolled", Page::PlayerAt("tiles_scrolled")),
            ("13_selection_loop", Page::PlayerSelection),
            ("13b_selection_slow", Page::PlayerAt("sel_slow")),
            ("13c_zoomed_out", Page::PlayerAt("zoomed_out")),
            ("13d_sections", Page::PlayerAt("sections")),
            ("13e_selection_track", Page::PlayerAt("sel_track")),
            ("13f_scrolled_lock", Page::PlayerAt("scrolled_lock")),
            ("13g_zoom", Page::PlayerAt("zoom")),
            ("13h_back_tip", Page::PlayerAt("back_tip")),
            ("14_controls", Page::PlayerControls),
            ("14b_controls_more", Page::PlayerAt("controls_more")),
            ("15_paused", Page::PlayerAt("paused")),
            ("16_toast", Page::PlayerAt("toast")),
            ("17_slowing_down", Page::PlayerAt("slowing")),
            ("20_settings_audio", Page::Settings("audio")),
            ("20_settings_tone", Page::Settings("tone")),
            ("20_settings_display", Page::Settings("display")),
            ("20_settings_practice", Page::Settings("practice")),
            ("20_settings_library", Page::Settings("library")),
            ("30_results", Page::Results),
            ("31_results_practice", Page::PlayerAt("results_practice")),
            ("32_no_audio", Page::PlayerAt("no_audio")),
        ];
        let own: [(&str, Page, ThemeName); 7] = [
            ("04_search", Page::Search, mid),
            ("12e_mg300_view", Page::Mg300View, mid),
            ("40_device_offer", Page::DeviceOffer, mid),
            ("41_note_widget", Page::NoteWidget, mid),
            ("00_home-daylight", Page::Home, ThemeName::Daylight),
            (
                "02_library_selected-graphite",
                Page::LibrarySelected,
                ThemeName::Graphite,
            ),
            ("10_player-daylight", Page::Player, ThemeName::Daylight),
        ];
        // each pointer shot straight after the shot of its page, as py_shots.py takes them (later, the player has
        // added plays to Home); only at 1440 x 900, where the Python shots' coordinates are
        let mut pointers: Vec<_> = if target == (1440, 900) {
            POINTER_SHOTS.to_vec()
        } else {
            Vec::new()
        };
        for (name, page) in py {
            plan.push(Shot {
                name: name.into(),
                theme: mid,
                page,
                pointer: None,
            });
            for (name, _, (x, y), down) in pointers.extract_if(.., |p| p.1 == page) {
                plan.push(Shot {
                    name: name.into(),
                    theme: mid,
                    page,
                    pointer: Some((x, y, down)),
                });
            }
        }
        for (name, page, theme) in own {
            plan.push(Shot {
                name: name.into(),
                theme,
                page,
                pointer: None,
            });
        }
        if let Some(th) = theme {
            plan.retain(|s| {
                let variant = s.name.ends_with("-daylight") || s.name.ends_with("-graphite");
                !variant && (s.page != Page::Gallery || s.theme == th)
            });
            for s in &mut plan {
                s.theme = th;
            }
        }
        // QUICKRS_SHOTS=40_device,20_settings_tone: only the shots whose names contain one of these
        if let Ok(only) = std::env::var("QUICKRS_SHOTS") {
            let want: Vec<&str> = only.split(',').map(str::trim).filter(|w| !w.is_empty()).collect();
            if !want.is_empty() {
                plan.retain(|s| want.iter().any(|w| s.name.contains(w)));
            }
        }
        let _ = std::fs::create_dir_all(&dir);
        Shots {
            dir,
            target,
            plan,
            i: 0,
            frames: 0,
            waiting: false,
            resize_tries: 0,
        }
    }

    pub fn current(&self) -> Option<&Shot> {
        self.plan.get(self.i)
    }

    pub fn index(&self) -> Option<usize> {
        (self.i < self.plan.len()).then_some(self.i)
    }

    /// Call at the end of every frame; `ready`: the page has what it shows (the library has been read).
    pub fn step(&mut self, ctx: &egui::Context, ready: bool) {
        ctx.request_repaint();
        let ppp = ctx.pixels_per_point();
        let size = ctx.content_rect().size() * ppp;
        let (tw, th) = (self.target.0 as f32, self.target.1 as f32);
        if ((size.x - tw).abs() > 1.0 || (size.y - th).abs() > 1.0) && self.resize_tries < 120 {
            if self.resize_tries.is_multiple_of(10) {
                ctx.send_viewport_cmd(ViewportCommand::InnerSize(egui::vec2(tw / ppp, th / ppp)));
            }
            self.resize_tries += 1;
            self.frames = 0;
            return;
        }
        for ev in ctx.input(|i| i.raw.events.clone()) {
            if let Event::Screenshot { user_data, image, .. } = ev {
                // (other captures, e.g. the backdrop under a panel, aren't ours)
                let Some(name) = user_data
                    .data
                    .as_ref()
                    .and_then(|d| d.downcast_ref::<String>())
                    .cloned()
                else {
                    continue;
                };
                let path = self.dir.join(format!("{name}.png"));
                let [w, h] = image.size;
                let rgba: Vec<u8> = image
                    .pixels
                    .iter()
                    .flat_map(|c| [c.r(), c.g(), c.b(), 255])
                    .collect();
                match image::RgbaImage::from_raw(w as u32, h as u32, rgba).map(|img| img.save(&path)) {
                    Some(Ok(())) => println!("{} ({w}x{h})", path.display()),
                    Some(Err(e)) => eprintln!("{}: {e}", path.display()),
                    None => eprintln!("{}: bad image", path.display()),
                }
                self.waiting = false;
                self.i += 1;
                self.frames = 0;
            }
        }
        if self.i >= self.plan.len() {
            ctx.send_viewport_cmd(ViewportCommand::Close);
            return;
        }
        if !ready {
            self.frames = 0;
            return;
        }
        self.frames += 1;
        // pages settle longer: covers decode in the background, rows open up
        let wait = if self.plan[self.i].page == Page::Gallery {
            12
        } else {
            90
        };
        if self.frames == wait && !self.waiting {
            ctx.send_viewport_cmd(ViewportCommand::Screenshot(UserData::new(
                self.plan[self.i].name.clone(),
            )));
            self.waiting = true;
        }
    }
}
