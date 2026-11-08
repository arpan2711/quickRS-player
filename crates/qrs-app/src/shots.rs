//! Screenshot mode (`quickrs --screenshots DIR`): sets the window to an exact physical size, shows each page in
//! each theme, lets it settle, captures the frame (eframe's screenshot command) and saves a PNG, then quits. Used
//! to compare the screens with the Python app's (P5.6, PLAN.md section 3). Run it with QUICKRS_DATA pointing at a
//! scratch folder (a config.json with library_dirs, and a copy of userdata.json) so nothing of yours changes; no
//! sound device is opened. QUICKRS_SHOTS (comma-separated parts of names) takes only some of the shots.
//!
//! QUICKRS_SHOT_SEQ="t0:t1:fps" (or a list "t1,t2,...") renders each shot as a frame sequence instead: the song held at t0, t0 + 1/fps, ...
//! t1 (`shotclock`, never the wall clock), one PNG a frame (`frame_00000.png` for a single shot, else
//! `<name>_00000.png`). With it, a name in QUICKRS_SHOTS that matches a shot exactly takes only that shot, and
//! QUICKRS_SHOT_JUDGE=hit|late|early|miss judges each note so as it reaches the now-line. `tools/hwclip.py` runs one
//! and puts it beside a reference video.

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
    /// The player with the Pedalboard tile first (on the pedalboard tone), and with its big view open (P14).
    PlayerBoard,
    BoardView,
    /// Home with the note widget drawn on it (its own window otherwise); with its drawer out, naming Am7 (P24.3).
    NoteWidget,
    NoteWidgetChords,
    /// Home with the first-time setup over it on a page (P23.14; `FirstSetup::staged`): "look", "guitar",
    /// "guitar_<step>" (a step of the guitar setup inside it), "latency", "widget", "folders", "done".
    Setup(&'static str),
    /// The library in a state: "menu" (the playlist menu), "newpl" (the name box), "fav", "loading", "licks" (the
    /// Saved licks page, with three made up when there are none), "lessons" (the Lessons page, a riff done and one
    /// under way when nothing's been started).
    Lib(&'static str),
    /// The player in a state (see `PlayerView::prepare_shot`).
    PlayerAt(&'static str),
    /// Home with the "run the setup again?" card of a start after an update (P37.15).
    HomeNew,
    /// Home with the AMP ROOM button staged: "play" (a level and a note), "hot" (clipping), "hover" (P22.1).
    HomeAmp(&'static str),
    /// The amp room (P22.2-P22.3): "rig", "sounds", "save", "editor", "jam"; recording (P23.9): "rec",
    /// "rec_paused", "rec_saved".
    Room(&'static str),
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
                | Page::PlayerBoard
                | Page::BoardView
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
    /// QUICKRS_SHOT_SEQ: the frames' song times, and the frame being captured while one runs.
    seq: Option<Vec<f64>>,
    seq_k: Option<u64>,
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
            ("14b_controls_sound", Page::PlayerAt("controls_sound")),
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
        let own: [(&str, Page, ThemeName); 127] = [
            // sight reading on the tab page (P44.1): notation over the tab, and notation only
            ("13p_tab_both", Page::PlayerAt("tab_both"), mid),
            ("13p_tab_staff", Page::PlayerAt("tab_staff"), mid),
            // the tuner's faces (P48.3), one launch for all five
            ("12f_tuner_arc", Page::PlayerAt("tuner_face_arc"), mid),
            ("12f_tuner_needle", Page::PlayerAt("tuner_face_needle"), mid),
            ("12f_tuner_strobe", Page::PlayerAt("tuner_face_strobe"), mid),
            ("12f_tuner_pedal", Page::PlayerAt("tuner_face_pedal"), mid),
            ("12f_tuner_digital", Page::PlayerAt("tuner_face_digital"), mid),
            ("20_settings_display_end", Page::Settings("display_end"), mid),
            // the highway's streak readout (D144): x4 on a 60-note streak, and Daylight
            ("13o_highway_streak", Page::PlayerAt("highway_streak"), mid),
            (
                "13o_highway_streak-daylight",
                Page::PlayerAt("highway_streak"),
                ThemeName::Daylight,
            ),
            // the highway's techniques (P37.6d): the notes ahead given one family each, to judge how they read
            ("13m_highway_bends", Page::PlayerAt("hwtech_bends"), mid),
            ("13m_highway_slides", Page::PlayerAt("hwtech_slides"), mid),
            ("13m_highway_mutes", Page::PlayerAt("hwtech_mutes"), mid),
            ("13m_highway_harmonics", Page::PlayerAt("hwtech_harmonics"), mid),
            ("13m_highway_tremolo", Page::PlayerAt("hwtech_tremolo"), mid),
            ("13m_highway_slap", Page::PlayerAt("hwtech_slap"), mid),
            ("13m_highway_taps", Page::PlayerAt("hwtech_taps"), mid),
            // the note size (D143): further, and closer
            ("13n_highway_size85", Page::PlayerAt("highway_size85"), mid),
            ("13n_highway_size150", Page::PlayerAt("highway_size150"), mid),
            ("13n_highway_size180", Page::PlayerAt("highway_size180"), mid),
            // the lamps' row centred under the tab, with the count-in lamp on and off (P37.11)
            ("37d_lamps_row", Page::PlayerAt("lamps"), mid),
            ("37e_lamps_countin_off", Page::PlayerAt("countin_off"), mid),
            (
                "37d_lamps_row-daylight",
                Page::PlayerAt("lamps"),
                ThemeName::Daylight,
            ),
            (
                "37e_lamps_countin_off-daylight",
                Page::PlayerAt("countin_off"),
                ThemeName::Daylight,
            ),
            // the footer's restart and review keys (P37.8), hovered, and the review key's menu
            ("37_restart_hover", Page::PlayerAt("restart_hover"), mid),
            ("37b_review_key_menu", Page::PlayerAt("review_key"), mid),
            ("37c_review_key_none", Page::PlayerAt("review_key_none"), mid),
            (
                "37_restart_hover-daylight",
                Page::PlayerAt("restart_hover"),
                ThemeName::Daylight,
            ),
            (
                "37b_review_key_menu-daylight",
                Page::PlayerAt("review_key"),
                ThemeName::Daylight,
            ),
            (
                "37c_review_key_none-daylight",
                Page::PlayerAt("review_key_none"),
                ThemeName::Daylight,
            ),
            // the controls' View tab (P35.1; Practise is 14_controls, Sound 14b), and the three in Daylight
            ("14c_controls_view", Page::PlayerAt("controls_view"), mid),
            ("14_controls-daylight", Page::PlayerControls, ThemeName::Daylight),
            (
                "14b_controls_sound-daylight",
                Page::PlayerAt("controls_sound"),
                ThemeName::Daylight,
            ),
            (
                "14c_controls_view-daylight",
                Page::PlayerAt("controls_view"),
                ThemeName::Daylight,
            ),
            // the run's review (P29.1)
            ("33_review", Page::PlayerAt("review"), mid),
            ("34_review_spot", Page::PlayerAt("review_spot"), mid),
            ("35_review_continuous", Page::PlayerAt("review_continuous"), mid),
            ("36_review_history", Page::PlayerAt("review_history"), mid),
            // setup (P20.2): Settings > Library's folders and keys
            ("91_settings_library_folders", Page::Settings("library"), mid),
            ("91b_settings_library_keys", Page::Settings("library_keys"), mid),
            (
                "91c_settings_library_keys-daylight",
                Page::Settings("library_keys"),
                ThemeName::Daylight,
            ),
            ("20b_settings_input_level", Page::Settings("audio_level"), mid),
            ("20d_settings_low_latency", Page::Settings("audio_lowlat"), mid),
            ("20c_settings_updates", Page::Settings("updates"), mid),
            // the settings redesign (P35, D92): the new pages, and each in Daylight
            ("20_settings_timing", Page::Settings("timing"), mid),
            ("20e_settings_tone_knobs", Page::Settings("tone_knobs"), mid),
            ("20_settings_about", Page::Settings("about"), mid),
            (
                "20_settings_sound-daylight",
                Page::Settings("sound"),
                ThemeName::Daylight,
            ),
            (
                "20_settings_timing-daylight",
                Page::Settings("timing"),
                ThemeName::Daylight,
            ),
            (
                "20_settings_tone-daylight",
                Page::Settings("tone"),
                ThemeName::Daylight,
            ),
            (
                "20_settings_display-daylight",
                Page::Settings("display"),
                ThemeName::Daylight,
            ),
            (
                "20_settings_library-daylight",
                Page::Settings("library"),
                ThemeName::Daylight,
            ),
            (
                "20_settings_about-daylight",
                Page::Settings("about"),
                ThemeName::Daylight,
            ),
            ("18_indicators", Page::PlayerAt("indicators"), mid),
            ("19_shortcuts", Page::PlayerAt("shortcuts"), mid),
            ("13h_tab_tools", Page::PlayerAt("tab_tools"), mid),
            ("13h_tab_tools_hover", Page::PlayerAt("tab_tools_hover"), mid),
            ("13h_tab_tools_rolled", Page::PlayerAt("tab_tools_rolled"), mid),
            (
                "13h_tab_tools-daylight",
                Page::PlayerAt("tab_tools"),
                ThemeName::Daylight,
            ),
            (
                "13h_tab_tools-graphite",
                Page::PlayerAt("tab_tools"),
                ThemeName::Graphite,
            ),
            ("13i_tab_full", Page::PlayerAt("tab_full"), mid),
            ("13j_tab_compact", Page::PlayerAt("tab_compact"), mid),
            ("13k_tab_shape", Page::PlayerAt("tab_shape"), mid),
            ("13l_highway", Page::PlayerAt("highway"), mid),
            ("13l_highway_inverted", Page::PlayerAt("highway_invert"), mid),
            ("13l_highway_full", Page::PlayerAt("highway_full"), mid),
            ("12e_tuner_edit", Page::PlayerAt("tuner_edit"), mid),
            (
                "13l_highway-daylight",
                Page::PlayerAt("highway"),
                ThemeName::Daylight,
            ),
            ("70_board_tile", Page::PlayerBoard, mid),
            ("70b_board_tile-daylight", Page::PlayerBoard, ThemeName::Daylight),
            ("70c_board_tile-graphite", Page::PlayerBoard, ThemeName::Graphite),
            ("71_board_view", Page::BoardView, mid),
            ("72_board_add", Page::PlayerAt("board_add"), mid),
            ("72b_board_sounds", Page::PlayerAt("board_sounds"), mid),
            ("04_search", Page::Search, mid),
            ("42_saved_licks", Page::Lib("licks"), mid),
            // the rank badges and the Random keys (P37.10)
            ("50_library_ranks", Page::Lib("best"), mid),
            ("51_library_difficulty", Page::Lib("difficulty"), mid),
            ("51b_library_easy", Page::Lib("easy"), mid),
            (
                "50_library_ranks-daylight",
                Page::Lib("best"),
                ThemeName::Daylight,
            ),
            ("50b_library_playlist", Page::Lib("playlist"), mid),
            (
                "50c_library_favourites-daylight",
                Page::Lib("fav"),
                ThemeName::Daylight,
            ),
            (
                "50d_saved_licks-daylight",
                Page::Lib("licks"),
                ThemeName::Daylight,
            ),
            ("43_lessons", Page::Lib("lessons"), mid),
            ("44_ear_mode", Page::PlayerAt("ear"), mid),
            ("49_note_by_note", Page::PlayerAt("nbn"), mid),
            (
                "49b_note_by_note_wrong-daylight",
                Page::PlayerAt("nbn_wrong"),
                ThemeName::Daylight,
            ),
            ("44b_lesson_started", Page::PlayerAt("lesson"), mid),
            ("45_coach_tuner", Page::PlayerAt("coach_tuner"), mid),
            ("46_coach_notes", Page::PlayerAt("coach_notes"), mid),
            ("47_coach_play", Page::PlayerAt("coach_play"), mid),
            ("48_coach_check", Page::PlayerAt("coach_check"), mid),
            (
                "46_coach_notes-daylight",
                Page::PlayerAt("coach_notes"),
                ThemeName::Daylight,
            ),
            ("12e_mg300_view", Page::Mg300View, mid),
            ("40_device_offer", Page::DeviceOffer, mid),
            ("41_note_widget", Page::NoteWidget, mid),
            ("41b_note_widget_chords", Page::NoteWidgetChords, mid),
            ("00_home-daylight", Page::Home, ThemeName::Daylight),
            (
                "02_library_selected-graphite",
                Page::LibrarySelected,
                ThemeName::Graphite,
            ),
            ("10_player-daylight", Page::Player, ThemeName::Daylight),
            ("93_home_amp", Page::HomeAmp("play"), ThemeName::Graphite),
            ("93b_home_amp_hot", Page::HomeAmp("hot"), ThemeName::Graphite),
            ("93c_home_amp_hover", Page::HomeAmp("hover"), ThemeName::Graphite),
            ("94_room", Page::Room("rig"), ThemeName::Graphite),
            ("94b_room_sounds", Page::Room("sounds"), ThemeName::Graphite),
            ("94c_room_save", Page::Room("save"), ThemeName::Graphite),
            ("94d_room_editor", Page::Room("editor"), ThemeName::Graphite),
            // recording what you hear (P23.9): the room and the player while recording, paused, saved; the folders
            ("95_room_recording", Page::Room("rec"), ThemeName::Graphite),
            (
                "95b_room_rec_paused",
                Page::Room("rec_paused"),
                ThemeName::Graphite,
            ),
            ("95c_room_rec_saved", Page::Room("rec_saved"), ThemeName::Graphite),
            // the room as one screen (P37.1): a capture as the silver unit, an empty one, the gear drawer, a bass
            // input, a modeller input, the room in a song's context, song tone explained; Daylight
            ("94ra_room_unit_silver", Page::Room("silver"), ThemeName::Graphite),
            (
                "94rb_room_no_capture",
                Page::Room("silver_empty"),
                ThemeName::Graphite,
            ),
            ("94rc_room_gear", Page::Room("gear"), ThemeName::Graphite),
            ("94rd_room_bass", Page::Room("bass"), ThemeName::Graphite),
            ("94re_room_modeller", Page::Room("modeller"), ThemeName::Graphite),
            ("94rf_room_song", Page::PlayerAt("room_song"), mid),
            ("94rg_room_song_explained", Page::PlayerAt("room_explain"), mid),
            ("94rh_room-daylight", Page::Room("rig"), ThemeName::Daylight),
            (
                "94ri_room_silver-daylight",
                Page::Room("silver"),
                ThemeName::Daylight,
            ),
            ("96_player_recording", Page::PlayerAt("recording"), mid),
            ("96b_player_rec_paused", Page::PlayerAt("rec_paused"), mid),
            ("96c_player_rec_saved", Page::PlayerAt("rec_saved"), mid),
            (
                "96d_player_recording-daylight",
                Page::PlayerAt("recording"),
                ThemeName::Daylight,
            ),
            // the lamp row's synth guitar and rec keys (P23.11): off is 10_player; on, being prepared
            ("98_player_song_synth", Page::PlayerAt("song_synth"), mid),
            (
                "98b_player_song_synth_prep",
                Page::PlayerAt("song_synth_prep"),
                mid,
            ),
            (
                "98c_player_song_synth-daylight",
                Page::PlayerAt("song_synth"),
                ThemeName::Daylight,
            ),
            (
                "97_settings_library_recordings",
                Page::Settings("library_rec"),
                mid,
            ),
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
        // Settings > Neural Amp Modeler (P37.15): not set up (its setup, each step), set up (the management page),
        // and Home's card after an update; Midnight and Daylight
        if crate::captures::ON {
            let nam: [(&str, Page); 10] = [
                ("20n_settings_nam", Page::Settings("nam_about")),
                ("20n_settings_nam_songs", Page::Settings("nam_songs")),
                ("20n_settings_nam_songs_yes", Page::Settings("nam_songs_yes")),
                ("20n_settings_nam_account", Page::Settings("nam_account")),
                ("20n_settings_nam_suggest", Page::Settings("nam_suggest")),
                ("20n_settings_nam_files", Page::Settings("nam_files")),
                ("20n_settings_nam_done", Page::Settings("nam_done")),
                ("20n_settings_nam_manage", Page::Settings("nam_manage")),
                ("20n_settings_nam_manage_open", Page::Settings("nam_manage_open")),
                ("20n_settings_nam_manage_low", Page::Settings("nam_manage_low")),
            ];
            for th in [mid, ThemeName::Daylight] {
                for (name, page) in nam {
                    plan.push(Shot {
                        name: if th == mid {
                            name.to_string()
                        } else {
                            format!("{name}-{}", th.key())
                        },
                        theme: th,
                        page,
                        pointer: None,
                    });
                }
            }
        }
        // TONE3000 downloads you can see (P38.3): waiting, coming in (a queue), measuring and done, failed; the
        // room's badge, folded and opened
        if crate::captures::ON {
            let dl: [(&str, Page); 7] = [
                ("20t_downloads_waiting", Page::Settings("nam_manage")),
                ("20t_downloads_rows", Page::Settings("nam_manage_low")),
                ("20t_downloads_progress", Page::Settings("nam_manage")),
                ("20t_downloads_done", Page::Home),
                ("20t_downloads_failed", Page::Settings("nam_manage")),
                ("20t_downloads_room", Page::Room("rig")),
                ("20t_downloads_room_open", Page::Room("rig")),
            ];
            for th in [mid, ThemeName::Daylight] {
                for (name, page) in dl {
                    plan.push(Shot {
                        name: if th == mid {
                            name.to_string()
                        } else {
                            format!("{name}-{}", th.key())
                        },
                        theme: th,
                        page,
                        pointer: None,
                    });
                }
            }
        }
        for th in [mid, ThemeName::Daylight] {
            plan.push(Shot {
                name: if th == mid {
                    "00w_home_whats_new".to_string()
                } else {
                    format!("00w_home_whats_new-{}", th.key())
                },
                theme: th,
                page: Page::HomeNew,
                pointer: None,
            });
        }
        // the guided guitar setup (P23.8): each step in Settings > Audio, and the first-run prompt, in every theme;
        // the calibration's instructions, measuring and settings, in the setup and on their own (P23.10)
        for th in [mid, ThemeName::Daylight, ThemeName::Graphite] {
            let tail = if th == mid {
                String::new()
            } else {
                format!("-{}", th.key())
            };
            for (k, step) in [
                ("95", "setup_input"),
                ("95b", "setup_output"),
                ("95c", "setup_level"),
                ("95d", "setup_kind"),
                ("95e", "setup_calibrate"),
                ("95e2", "setup_measure"),
                ("95e3", "setup_settings"),
                ("95e4", "setup_settingsfold"),
                ("95f", "setup_notes"),
                // (the play-along, P38.4: fresh, listening, playing, its result; the fold open)
                ("95h", "setup_timing"),
                ("95h0a", "setup_playlisten"),
                ("95h0b", "setup_playing"),
                ("95h0c", "setup_playresult"),
                ("95h1", "setup_timingclicks"),
                ("95h2", "setup_timingmeasure"),
                ("95h2b", "setup_timingscreen"),
                ("95h3", "setup_timingresult"),
                ("95g", "setup_done"),
                ("97", "calibrate_calibrate"),
                ("97b", "calibrate_measure"),
                ("97c", "calibrate_settings"),
                ("97d", "timingtool_fresh"),
                ("97d2", "timingtool_result"),
                ("97d3", "timingtool_playresult"),
            ] {
                plan.push(Shot {
                    name: format!("{k}_{step}{tail}"),
                    theme: th,
                    page: Page::Settings(step),
                    pointer: None,
                });
            }
        }
        // the first-time setup (P23.14): every page, and the guitar setup's steps inside it, in every theme
        for th in [mid, ThemeName::Daylight, ThemeName::Graphite] {
            let tail = if th == mid {
                String::new()
            } else {
                format!("-{}", th.key())
            };
            for (k, what) in [
                ("90", "look"),
                ("90b", "guitar"),
                ("90c", "guitar_input"),
                ("90d", "guitar_calibrate"),
                ("90e1", "guitar_timing"),
                ("90e", "guitar_done"),
                ("90e3", "nam"),
                ("90e4", "nam_yes"),
                ("90f", "latency"),
                ("90g", "widget"),
                ("90h", "folders"),
                ("90i", "done"),
            ] {
                if what.starts_with("nam") && !crate::captures::ON {
                    continue;
                }
                if what == "latency" && !cfg!(windows) {
                    continue;
                }
                plan.push(Shot {
                    name: format!("{k}_first_setup_{what}{tail}"),
                    theme: th,
                    page: Page::Setup(what),
                    pointer: None,
                });
            }
        }
        // the results card's close key, the page after closing it, and the header's transpose at 0 and +2
        // (P23.12), in every theme
        for th in [mid, ThemeName::Daylight, ThemeName::Graphite] {
            let tail = if th == mid {
                String::new()
            } else {
                format!("-{}", th.key())
            };
            let mut shots = vec![
                ("30c_results_closed", Page::PlayerAt("results_closed")),
                ("99_header_transpose", Page::Player),
                ("99b_header_transpose_up2", Page::PlayerAt("transpose2")),
            ];
            if th != mid {
                shots.insert(0, ("30_results", Page::Results)); // (midnight's is in the list above)
            }
            for (name, page) in shots {
                plan.push(Shot {
                    name: format!("{name}{tail}"),
                    theme: th,
                    page,
                    pointer: None,
                });
            }
        }
        // the amp room polish (P37.13): its states in every theme; QUICKRS_ROOM_UNITS=1 adds the open unit of every
        // model (64 shots a theme, so not by default)
        for th in [mid, ThemeName::Graphite, ThemeName::Daylight] {
            let tail = format!("-{}", th.key());
            for what in [
                "rig",
                "scenes",
                "empty",
                "ab",
                "gate",
                "silver",
                "gear",
                "bass",
                "cab",
                "sounds",
                "catalogue",
                "catalogue_amps",
                "catalogue_search",
                "catalogue_captures",
                "info",
                "info_capture",
            ] {
                plan.push(Shot {
                    name: format!("38_room_{what}{tail}"),
                    theme: th,
                    page: Page::Room(what),
                    pointer: None,
                });
            }
            if std::env::var("QUICKRS_ROOM_UNITS").is_ok() {
                for m in qrs_audio::board::catalog::MODELS {
                    let what: &'static str = Box::leak(format!("unit:{}", m.id).into_boxed_str());
                    plan.push(Shot {
                        name: format!("38_unit_{}{tail}", m.id),
                        theme: th,
                        page: Page::Room(what),
                        pointer: None,
                    });
                }
            }
        }
        // the score tile on a streak (P37.7): each tier, in every theme
        for th in [mid, ThemeName::Daylight, ThemeName::Graphite] {
            let tail = if th == mid {
                String::new()
            } else {
                format!("-{}", th.key())
            };
            for what in [
                "streak_10",
                "streak_20",
                "streak_50",
                "streak_100",
                "streak_200",
                "streak_500",
                "streak_1000",
            ] {
                plan.push(Shot {
                    name: format!("37_{what}{tail}"),
                    theme: th,
                    page: Page::PlayerAt(what),
                    pointer: None,
                });
            }
        }
        // lessons v2's and v3's page and cards (P16, P17), in every theme
        for th in [ThemeName::Daylight, ThemeName::Graphite] {
            plan.push(Shot {
                name: format!("43_lessons-{}", th.key()),
                theme: th,
                page: Page::Lib("lessons"),
                pointer: None,
            });
        }
        let coach: [(&str, &'static str); 25] = [
            ("45_coach_tuner", "coach_tuner"),
            ("47b_coach_slower", "coach_slower"),
            ("47d_coach_learn_done", "coach_learn_done"),
            // lessons v3 (P17): practise, a take, a callout, a theory item's pages
            ("80_coach_practise", "coach_practise"),
            ("80b_coach_practise_nbn", "coach_practise_nbn"),
            ("81_coach_record", "coach_record"),
            ("81b_coach_record_result", "coach_record_result"),
            ("82_coach_callout", "coach_callout"),
            ("84_theory_key", "coach_theory_key"),
            ("84b_theory_chords", "coach_theory_chords"),
            ("84c_theory_scale", "coach_theory_scale"),
            ("84d_theory_emphasis", "coach_theory_emphasis"),
            ("84e_theory_find", "coach_theory_find"),
            ("84f_theory_jam", "coach_theory_jam"),
            // takes (P17.11-P17.13): analysing, the result card and "your tab", in a lesson and out; new here
            ("85_take_analysing", "take_analysing"),
            ("85b_take_result", "take_result"),
            ("86_coach_record_analysing", "coach_record_analysing"),
            ("86b_coach_record_report", "coach_record_report"),
            ("87_new_here_lead", "coach_whole_lead"),
            ("87b_new_here_rhythm", "coach_whole_rhythm"),
            ("47e_coach_see", "coach_see"),
            ("47f_coach_run", "coach_run"),
            ("47g_coach_jam", "coach_jam"),
            ("47h_coach_scale_done", "coach_done"),
            ("47i_coach_play", "coach_play"),
        ];
        for (name, what) in coach {
            for th in [ThemeName::Daylight, ThemeName::Graphite, mid] {
                if th == mid && matches!(name, "45_coach_tuner" | "47i_coach_play") {
                    continue; // (already in the list above)
                }
                plan.push(Shot {
                    name: if th == mid {
                        name.to_string()
                    } else {
                        format!("{name}-{}", th.key())
                    },
                    theme: th,
                    page: Page::PlayerAt(what),
                    pointer: None,
                });
            }
        }
        // without the `lessons` feature (the shared build, D50) there's no Lessons page, lesson or coach to shoot
        if !cfg!(feature = "lessons") {
            plan.retain(|s| {
                !s.name.contains("lesson")
                    && !matches!(s.page, Page::Lib("lessons") | Page::PlayerAt("lesson"))
                    && !matches!(s.page, Page::PlayerAt(w) if w.starts_with("coach_"))
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
                // (a sequence takes the shot named exactly, when there is one: 13l_highway, not its variants)
                let exact = crate::shotclock::seq_env().is_some()
                    && plan.iter().any(|s| want.contains(&s.name.as_str()));
                plan.retain(|s| {
                    if exact {
                        want.contains(&s.name.as_str())
                    } else {
                        want.iter().any(|w| s.name.contains(w))
                    }
                });
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
            seq: crate::shotclock::seq_env(),
            seq_k: None,
        }
    }

    /// A sequence frame's file name.
    fn frame_name(&self, k: u64) -> String {
        if self.plan.len() == 1 {
            format!("frame_{k:05}")
        } else {
            format!("{}_{k:05}", self.plan[self.i].name)
        }
    }

    /// Put the sequence's clock on frame k and ask for it to be captured (the next frame drawn, which reads it).
    fn seq_frame(&mut self, ctx: &egui::Context, k: u64) {
        let Some(&t) = self.seq.as_ref().and_then(|s| s.get(k as usize)) else {
            return;
        };
        crate::shotclock::set(Some(t));
        self.seq_k = Some(k);
        ctx.send_viewport_cmd(ViewportCommand::Screenshot(UserData::new(self.frame_name(k))));
        self.waiting = true;
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
                // a sequence: on to its next frame until the last
                if let (Some(k), Some(seq)) = (self.seq_k, &self.seq)
                    && ((k + 1) as usize) < seq.len()
                {
                    self.seq_frame(ctx, k + 1);
                    continue;
                }
                self.seq_k = None;
                crate::shotclock::set(None);
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
            if self.seq.is_some() {
                self.seq_frame(ctx, 0);
                return;
            }
            ctx.send_viewport_cmd(ViewportCommand::Screenshot(UserData::new(
                self.plan[self.i].name.clone(),
            )));
            self.waiting = true;
        }
    }
}
