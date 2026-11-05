//! What every screen shares: the data folder, config.json and userdata.json, the keys, the library (scanned in the
//! background, grouped into one row per song), the covers, the audio engine and the previewer.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Instant;

use eframe::egui;
use qrs_audio::{Audio, AudioPrefs};
use qrs_formats::Keys;
use qrs_library::{Config, DataDir, Group, Lesson, Library, Prefer, SongEntry, UserData, config, group};
use qrs_ui::theme::{self, ThemeName};

use crate::covers::Covers;
use crate::preview::Previewer;

/// How the app uses sound.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AudioMode {
    /// The sound devices (the normal app).
    Devices,
    /// The engine running on no devices: the silent timer, no input. Nothing is heard.
    Silent,
    /// Screenshot mode: the engine on the stand-in guitar (the Python screenshot tool's: a steady Eb3 on "NUX Audio
    /// (ASIO)"), heard by no one. Otherwise as with real devices, so the shots show what a connected guitar shows.
    Shots,
    /// No engine at all.
    Off,
}

pub struct AppState {
    pub ctx: egui::Context,
    pub data: DataDir,
    pub config: Config,
    pub userdata: UserData,
    pub keys: Keys,
    /// What the key lookup found, and the files it looks at (watched: songs are read again when keys arrive).
    pub key_state: crate::keyfile::KeyState,
    pub key_files: Vec<PathBuf>,
    key_watch: Arc<std::sync::atomic::AtomicBool>,
    /// A scan asked for while one was running (folders or keys changed): started when it's done.
    scan_pending: bool,
    /// There was no config.json at the start (the first run: the first-time setup opens, P23.14).
    pub first_run: bool,
    /// Home shows "run the setup again?" this start: an update to this version (P37.15, `whatsnew`).
    pub whats_new: bool,
    pub library: Arc<Library>,
    /// The folders scanned (the configured ones, plus a feedpak folder next to a dlc folder).
    pub dirs: Vec<PathBuf>,
    pub songs: Arc<Vec<SongEntry>>,
    pub groups: Vec<Group>,
    /// How many times `groups` has been rebuilt, and the last few rebuilds' maps from the rows before to the rows
    /// after (by song file): a view holding a row number across one finds the row again with `follow`.
    pub regroups: u64,
    moves: std::collections::VecDeque<Vec<Option<usize>>>,
    /// Songs from files dropped on the window that aren't in the library folders (listed for this session).
    loose: Vec<SongEntry>,
    /// Whether the input profile last applied was a microphone's (see `input_profile`).
    mic_applied: Option<bool>,
    lib_version: u64,
    /// Bumped whenever `groups` or the user data change (views cache what they count against it).
    pub version: u64,
    pub covers: Covers,
    pub audio: Option<Audio>,
    pub audio_mode: AudioMode,
    pub previewer: Previewer,
    pub toast: Option<(String, Instant)>,
    /// Recording what you hear (P23.9): one recorder for the song player and the amp room.
    pub rec: crate::recording::Recording,
    /// The lessons (P13.3): the built-in ones and those in `<data>/lessons`, by number.
    pub lessons: Vec<Arc<Lesson>>,
    /// Checks for a newer release, and installs it (P20.1, D36).
    pub updater: crate::update::Updater,
    /// The NUX MG-300 over USB MIDI, started the first time something shows it (the MG-300 tile); a simulated
    /// pedal in silent mode, so screenshots and night tests never touch the real one.
    #[cfg(feature = "nux")]
    pedal: Option<qrs_mg300::Pedal>,
    /// The real pedal's control port (P11.6, D24): other programs change the pedal through `pedal`.
    #[cfg(feature = "nux")]
    mg300_control: Option<qrs_mg300::control::Control>,
}

impl AppState {
    /// `audio`: the sound devices, or the engine on no devices (silent: screenshots, tests at night).
    pub fn new(ctx: &egui::Context, audio_mode: AudioMode) -> AppState {
        let audio = audio_mode;
        let data = DataDir::locate();
        crate::applog::init(data.cache());
        // the first start: the Python app's settings, scores, keys and covers (P8.2)
        let migrated = if qrs_library::migrate::is_fresh(&data) {
            let exe = std::env::current_exe()
                .ok()
                .and_then(|p| p.parent().map(|d| d.to_path_buf()));
            qrs_library::migrate::find_python_app(exe.as_deref())
                .and_then(|from| qrs_library::migrate::migrate(&from, &data).ok())
                .filter(|r| !r.copied.is_empty() || r.covers > 0)
        } else {
            None
        };
        let first_run = !data.config().is_file();
        let mut config = Config::load(data.config());
        // the Python app's sound devices, as quickRS's choice (once)
        if config.get("audio").is_none()
            && let Some(prefs) = python_audio_prefs(&config)
        {
            config.set("audio", serde_json::to_value(&prefs).unwrap_or_default());
            let _ = config.save();
        }
        let userdata = UserData::load(data.userdata());
        let key_files = crate::keyfile::candidates(&data.root, &exe_dirs());
        let (keys, key_state) = crate::keyfile::load(&key_files);
        log_keys(&key_state);
        let key_watch = {
            let ctx = ctx.clone();
            crate::keyfile::watch(key_files.clone(), move || ctx.request_repaint())
        };
        theme::set_theme(ThemeName::parse(&config.str("theme", "midnight")));
        qrs_ui::knobs::set_style(qrs_ui::knobs::KnobStyle::parse(&config.str("knobs", "amp")));
        theme::set_string_colors(config.bool("string_colors", false));
        let dirs = config.scan_dirs();
        let library = Library::new();
        library.start_scan(keys.clone(), dirs.clone(), data.index());
        let audio = (audio != AudioMode::Off).then(|| {
            let prefs: AudioPrefs = if audio == AudioMode::Silent {
                AudioPrefs {
                    input: qrs_audio::Choice::Off,
                    output: qrs_audio::Choice::Off,
                    use_rs_setup: false,
                    ..Default::default()
                }
            } else if audio == AudioMode::Shots {
                use qrs_audio::backend::{Backend, null};
                null::enable_stand_in();
                let stand_in = qrs_audio::DevicePref {
                    backend: Backend::Null,
                    id: null::STAND_IN.into(),
                    name: null::stand_in_name(),
                };
                AudioPrefs {
                    input: qrs_audio::Choice::Device(stand_in.clone()),
                    channel: Some(0),
                    output: qrs_audio::Choice::Device(stand_in),
                    use_rs_setup: false,
                    ..Default::default()
                }
            } else {
                config
                    .get("audio")
                    .and_then(|v| serde_json::from_value(v.clone()).ok())
                    .unwrap_or_default()
            };
            // (RS's own setup is found next to its dlc folder, searched for songs or not)
            let mut setup_dirs = dirs.clone();
            setup_dirs.extend(config::rs_dlc_dirs());
            let a = Audio::start(prefs, setup_dirs);
            a.engine.set_song_gain(config.f64("volume", 0.8) as f32);
            a.engine.set_guitar_gain(config.f64("guitar_volume", 0.8) as f32);
            a.engine.set_monitor(config.bool("monitor", true));
            a.engine.set_tone(crate::tones::current(&config));
            a
        });
        let mut previewer = Previewer::new(audio.as_ref().map(|a| a.engine.clone()), keys.clone());
        if audio_mode == AudioMode::Shots {
            previewer.held = Some((9.0, 30.0));
        }
        let covers = Covers::new(ctx.clone(), data.art(), keys.clone());
        let lessons = load_lessons(&data);
        // a newer release? (in the background, at most once a day; not in the test and screenshot modes)
        let mut updater = crate::update::Updater::new(Some(ctx.clone()));
        if audio_mode == AudioMode::Devices {
            updater.auto_check(&mut config);
        }
        AppState {
            ctx: ctx.clone(),
            data,
            config,
            userdata,
            keys,
            key_state,
            key_files,
            key_watch,
            scan_pending: false,
            first_run,
            whats_new: false,
            library,
            dirs,
            songs: Arc::new(Vec::new()),
            groups: Vec::new(),
            regroups: 0,
            moves: Default::default(),
            loose: Vec::new(),
            mic_applied: None,
            lib_version: u64::MAX,
            version: 0,
            covers,
            audio,
            audio_mode,
            previewer,
            rec: Default::default(),
            toast: migrated.map(|r| {
                (
                    format!("Brought over {} from the Python app", r.summary()),
                    Instant::now(),
                )
            }),
            lessons,
            updater,
            #[cfg(feature = "nux")]
            pedal: None,
            #[cfg(feature = "nux")]
            mg300_control: None,
        }
    }

    /// Tests: a state on a scratch data folder with one song in its library (no scan, no devices, no engine: the
    /// caller gives the player an engine on the null devices). Never the owner's data.
    #[cfg(test)]
    pub fn for_test(ctx: &egui::Context, root: PathBuf, entry: SongEntry) -> AppState {
        let data = DataDir::at(root);
        let config = Config::load(data.config());
        let userdata = UserData::load(data.userdata());
        let keys = Keys::default();
        let canonical = format!("{}|{}", entry.artist, entry.title).to_lowercase();
        AppState {
            ctx: ctx.clone(),
            config,
            userdata,
            keys: keys.clone(),
            key_state: crate::keyfile::KeyState::Missing,
            key_files: Vec::new(),
            key_watch: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            scan_pending: false,
            first_run: false,
            whats_new: false,
            library: Library::new(),
            dirs: Vec::new(),
            songs: Arc::new(vec![entry]),
            groups: vec![Group {
                members: vec![0],
                preferred: 0,
                canonical,
            }],
            regroups: 0,
            moves: Default::default(),
            loose: Vec::new(),
            mic_applied: None,
            lib_version: u64::MAX,
            version: 0,
            covers: Covers::new(ctx.clone(), data.art(), keys.clone()),
            audio: None,
            audio_mode: AudioMode::Off,
            previewer: Previewer::new(None, keys),
            toast: None,
            rec: Default::default(),
            lessons: Vec::new(),
            updater: crate::update::Updater::new(None),
            data,
            #[cfg(feature = "nux")]
            pedal: None,
            #[cfg(feature = "nux")]
            mg300_control: None,
        }
    }

    #[cfg(feature = "nux")]
    pub fn pedal(&mut self) -> &qrs_mg300::Pedal {
        let ctx = self.ctx.clone();
        let silent = self.audio_mode != AudioMode::Devices;
        let shots = self.audio_mode == AudioMode::Shots;
        let fresh = self.pedal.is_none();
        let pedal = self.pedal.get_or_insert_with(|| {
            let wake: Box<dyn Fn(&qrs_mg300::Snapshot) + Send + Sync> =
                Box::new(move |_| ctx.request_repaint());
            if silent {
                // the screenshots' pedal is the Python tool's (preset 1C, "Teen Spirit")
                let sim = if shots {
                    qrs_mg300::sim::SimPedal::python_shots()
                } else {
                    qrs_mg300::sim::SimPedal::new()
                };
                qrs_mg300::Pedal::with_transport(sim.opener(), qrs_mg300::Timing::default(), Some(wake))
            } else {
                qrs_mg300::Pedal::start(Some(wake))
            }
        });
        if fresh && !silent {
            let file = self.data.cache().join("mg300-control.json");
            match qrs_mg300::control::Control::start(pedal.handle(), &file) {
                Ok(c) => self.mg300_control = Some(c),
                Err(e) => crate::applog::write(&format!("MG-300 control port: {e}")),
            }
        }
        pedal
    }

    pub fn prefer(&self) -> Prefer {
        Prefer::parse(&self.config.str("prefer", "")).unwrap_or(if self.keys.psarc.is_some() {
            Prefer::Psarc
        } else {
            Prefer::Feedpak
        })
    }

    /// Pick up a new song list from the scan (call once a frame).
    pub fn refresh(&mut self) {
        self.input_profile();
        if self.key_watch.swap(false, Ordering::AcqRel) {
            self.reload_keys();
        }
        if self.scan_pending && !self.scanning() {
            self.scan_pending = false;
            self.library
                .start_scan(self.keys.clone(), self.dirs.clone(), self.data.index());
        }
        if self.scanning() {
            // the scan runs on its own threads: keep looking until it's done (pages may be idle meanwhile)
            self.ctx
                .request_repaint_after(std::time::Duration::from_millis(100));
        }
        let v = self.library.version.load(Ordering::Acquire);
        if v == self.lib_version {
            return;
        }
        self.ctx.request_repaint();
        self.lib_version = v;
        let old = self.songs.clone();
        self.songs = self.library.snapshot();
        if !self.loose.is_empty() {
            let mut all = (*self.songs).clone();
            for e in &self.loose {
                if !all.iter().any(|x| x.path == e.path && x.song_key == e.song_key) {
                    all.push(e.clone());
                }
            }
            self.songs = Arc::new(all);
        }
        self.set_groups(old);
    }

    /// The guitar input is a microphone (an acoustic guitar played to a webcam or laptop mic, RS's
    /// "microphone" mode, PLAN.md section 4.1): as you set the input's kind (Settings > Audio, D33), else by the
    /// device's kind.
    /// What the input now listening plays (P32.1): a guitar unless Settings > Audio says bass.
    pub fn instrument(&self) -> qrs_analysis::instrument::Instrument {
        crate::calibrate::instrument(self)
    }

    pub fn mic_input(&self) -> bool {
        use crate::calibrate::{self, Kind};
        let device = || {
            self.audio
                .as_ref()
                .and_then(|a| a.plan().input)
                .is_some_and(|i| i.kind == qrs_audio::devices::rank::InputKind::Microphone)
        };
        match calibrate::current_input(self).map(|(id, _)| calibrate::load(&self.config, &id).kind) {
            Some(Kind::Mic) => true,
            Some(Kind::Dry | Kind::Modeller) => false,
            _ => device(),
        }
    }

    /// The setting for hearing your guitar: a microphone has its own, off at first (a mic heard through the
    /// speakers feeds back).
    pub fn monitor_key(&self) -> (&'static str, bool) {
        if self.mic_input() {
            ("monitor_mic", false)
        } else {
            ("monitor", true)
        }
    }

    /// When the input changes between a guitar input and a microphone: its monitoring, and how loud a pluck must
    /// be (a mic hears the room too).
    fn input_profile(&mut self) {
        let mic = self.mic_input();
        if self.mic_applied == Some(mic) {
            return;
        }
        let first = self.mic_applied.is_none();
        self.mic_applied = Some(mic);
        if let Some(a) = &self.audio {
            let (key, dflt) = self.monitor_key();
            a.engine.set_monitor(self.config.bool(key, dflt));
        }
        crate::calibrate::apply(self); // (the pluck threshold: this input's, else a mic's or a guitar's)
        if mic && !first {
            self.say("A microphone is the input: hearing it through the speakers is off (M turns it on)");
        }
    }

    /// Group the songs again (after the psarc / feedpak preference changed).
    pub fn regroup(&mut self) {
        self.set_groups(self.songs.clone());
    }

    /// Rebuild the rows from `songs` (it was `old`), keeping the map from the old rows to the new.
    fn set_groups(&mut self, old: Arc<Vec<SongEntry>>) {
        let groups = group(&self.songs, self.prefer());
        let mut row: HashMap<(&str, &str), usize> = HashMap::new();
        for (g, grp) in groups.iter().enumerate() {
            for &i in &grp.members {
                let e = &self.songs[i];
                row.insert((e.path.as_str(), e.song_key.as_str()), g);
            }
        }
        let moved = self
            .groups
            .iter()
            .map(|grp| {
                let e = old.get(grp.preferred)?;
                row.get(&(e.path.as_str(), e.song_key.as_str())).copied()
            })
            .collect();
        self.groups = groups;
        if self.moves.len() == 16 {
            self.moves.pop_front();
        }
        self.moves.push_back(moved);
        self.regroups += 1;
        self.version += 1;
    }

    /// Row `g` of the rows as they were after rebuild `since` (a `regroups` value), in today's rows; None when its
    /// song has gone (or the map is too old to say).
    pub fn follow(&self, g: usize, since: u64) -> Option<usize> {
        let back = self.regroups.checked_sub(since)? as usize;
        if back > self.moves.len() {
            return None;
        }
        let mut g = g;
        for m in self.moves.range(self.moves.len() - back..) {
            g = (*m.get(g)?)?;
        }
        Some(g)
    }

    /// Read the song folders again from scratch (F5).
    pub fn rescan(&self) {
        self.library
            .rescan(self.keys.clone(), self.dirs.clone(), self.data.index());
    }

    /// A song file dropped on the window: its row (added for this session when it isn't in the library).
    pub fn add_dropped(&mut self, path: &std::path::Path) -> Result<usize, String> {
        let key = path.to_string_lossy().into_owned();
        let row_of = |st: &AppState, i: usize| st.groups.iter().position(|g| g.members.contains(&i));
        if let Some(i) = self.songs.iter().position(|e| e.path == key)
            && let Some(g) = row_of(self, i)
        {
            return Ok(g);
        }
        let entries = qrs_library::scan::read_entries(path, &self.keys);
        let Some(first) = entries.first() else {
            return Err("not a song file".into());
        };
        if !first.error.is_empty() && !first.needs_keys {
            return Err(first.error.clone());
        }
        self.loose.extend(entries.iter().cloned());
        let start = self.songs.len();
        let mut all = (*self.songs).clone();
        all.extend(entries);
        self.songs = Arc::new(all);
        self.regroup();
        row_of(self, start).ok_or_else(|| "couldn't list it".into())
    }

    /// A folder of songs added to the library for good (Settings > Library's Add folder, the first-time setup, or dropped on
    /// the window): saved in config.json's library_dirs, and read now.
    pub fn add_folder(&mut self, dir: &std::path::Path) -> Result<String, String> {
        if self.dirs.iter().any(|d| config::same_dir(d, dir)) {
            return Ok(format!("{} is already in the library", dir.display()));
        }
        self.config.add_dir(dir);
        self.config
            .save()
            .map_err(|e| format!("couldn't save config.json: {e}"))?;
        self.apply_dirs();
        Ok(format!(
            "Added {} to the library: reading its songs",
            dir.display()
        ))
    }

    /// A folder taken out of the library (Settings > Library's Remove); its songs go from the list.
    pub fn remove_folder(&mut self, dir: &std::path::Path) -> Result<String, String> {
        self.config.remove_dir(dir);
        self.config
            .save()
            .map_err(|e| format!("couldn't save config.json: {e}"))?;
        self.apply_dirs();
        Ok(format!("Removed {} from the library", dir.display()))
    }

    /// Search RS's own dlc folder for songs, or not (feedpaks only).
    pub fn set_search_dlc(&mut self, on: bool) {
        self.config.set(config::SEARCH_RS_DLC, on);
        self.save_config();
        self.apply_dirs();
    }

    /// The folders to scan worked out again from config.json, and read.
    fn apply_dirs(&mut self) {
        self.dirs = self.config.scan_dirs();
        self.scan_soon();
    }

    /// Read the song folders (what changed, and songs waiting for keys) as soon as no scan is running.
    pub fn scan_soon(&mut self) {
        if self.scanning() {
            self.scan_pending = true;
        } else {
            self.library
                .start_scan(self.keys.clone(), self.dirs.clone(), self.data.index());
        }
        self.ctx.request_repaint();
    }

    /// Look for the keys again (a keys file appeared or changed, or keys were saved in Settings). New keys go to
    /// the covers and previews, and songs that needed them are read again.
    pub fn reload_keys(&mut self) {
        let (keys, state) = crate::keyfile::load(&self.key_files);
        let changed = !keys.same_keys(&self.keys);
        if state != self.key_state {
            log_keys(&state);
        }
        self.key_state = state;
        if !changed {
            return;
        }
        let reread = crate::keyfile::needs_reread(&self.keys, &keys, &self.songs);
        self.keys = keys;
        self.covers.set_keys(self.keys.clone());
        self.previewer.set_keys(self.keys.clone());
        if reread {
            // songs dropped on the window this session that needed keys
            let waiting: Vec<String> = self
                .loose
                .iter()
                .filter(|e| e.needs_keys)
                .map(|e| e.path.clone())
                .collect();
            for path in waiting {
                let fresh = qrs_library::scan::read_entries(std::path::Path::new(&path), &self.keys);
                self.loose.retain(|e| e.path != path);
                self.loose.extend(fresh);
            }
            self.scan_soon();
            self.say("RS keys found: reading the psarc songs again");
        }
    }

    pub fn scanning(&self) -> bool {
        self.library.scanning.load(Ordering::Acquire)
    }

    /// The source that plays for a row.
    pub fn song(&self, g: usize) -> &SongEntry {
        &self.songs[self.groups[g].preferred]
    }

    /// The row a song file is on now (rows are rebuilt each time the library is read, so a row number kept while a
    /// song loads or plays can name another song by then).
    pub fn row_of(&self, e: &SongEntry) -> Option<usize> {
        let i = self
            .songs
            .iter()
            .position(|x| x.path == e.path && x.song_key == e.song_key)?;
        self.groups.iter().position(|g| g.members.contains(&i))
    }

    /// Every id a row's history is under.
    pub fn ids(&self, g: usize) -> Vec<String> {
        let grp = &self.groups[g];
        self.userdata.ids_for(&grp.canonical, &grp.ids(&self.songs))
    }

    /// Rows that can be listed (songs that read, or psarcs that only need keys).
    pub fn listed(&self, g: usize) -> bool {
        let s = self.song(g);
        s.error.is_empty() || s.needs_keys
    }

    pub fn say(&mut self, msg: impl Into<String>) {
        self.toast = Some((msg.into(), Instant::now()));
    }

    /// Read the lessons again (the Lessons page's F5: a lesson file added or changed).
    pub fn reload_lessons(&mut self) {
        self.lessons = load_lessons(&self.data);
        self.version += 1;
    }

    pub fn lesson(&self, id: &str) -> Option<Arc<Lesson>> {
        self.lessons.iter().find(|l| l.id == id).cloned()
    }

    /// The row a riff's song is on: the row with its canonical id, else the same artist with one title inside the
    /// other (a song named a little differently).
    pub fn riff_row(&self, r: &qrs_library::Riff) -> Option<usize> {
        let canon = r.canonical();
        let rows = || (0..self.groups.len()).filter(|&g| self.listed(g));
        rows().find(|&g| self.groups[g].canonical == canon).or_else(|| {
            rows().find(|&g| {
                let e = self.song(g);
                r.is_song(&e.artist, &e.display_title())
            })
        })
    }

    /// A message that's also an error worth keeping (quickrs.log).
    pub fn say_error(&mut self, msg: impl Into<String>) {
        let m = msg.into();
        crate::applog::write(&m);
        self.say(m);
    }

    pub fn save_config(&self) {
        let _ = self.config.save();
    }

    /// The pick in the sidebar: the three themes in turn, Midnight, Graphite, Daylight (remembered in config.json).
    pub fn toggle_theme(&mut self) {
        let next = match theme::theme() {
            ThemeName::Midnight => ThemeName::Graphite,
            ThemeName::Graphite => ThemeName::Daylight,
            ThemeName::Daylight => ThemeName::Midnight,
        };
        self.apply_theme(next);
    }

    pub fn apply_theme(&mut self, name: ThemeName) {
        theme::set_theme(name);
        crate::setup::set_theme(&mut self.config, name);
        self.save_config();
    }
}

/// The Python app's device settings (`input_device`, `output_device`, `input_channel`, `exclusive_output`) as
/// quickRS's (None when there are none). Its ASIO devices were named "<driver> (ASIO)", the others by their
/// WASAPI name, which the device list matches by name.
pub fn python_audio_prefs(cfg: &Config) -> Option<AudioPrefs> {
    use qrs_audio::backend::Backend;
    use qrs_audio::{Choice, DevicePref};
    let known = [
        "input_device",
        "output_device",
        "input_channel",
        "exclusive_output",
    ];
    if !known.iter().any(|k| cfg.get(k).is_some()) {
        return None;
    }
    let dev = |name: &str| match name.strip_suffix(" (ASIO)") {
        Some(drv) => Choice::Device(DevicePref {
            backend: Backend::Asio,
            id: drv.into(),
            name: drv.into(),
        }),
        None => Choice::Device(DevicePref {
            backend: Backend::Wasapi,
            id: String::new(),
            name: name.into(),
        }),
    };
    let mut p = AudioPrefs::default();
    if let Some(n) = cfg
        .get("input_device")
        .and_then(|v| v.as_str())
        .filter(|n| !n.is_empty())
    {
        p.input = dev(n);
    }
    if let Some(n) = cfg
        .get("output_device")
        .and_then(|v| v.as_str())
        .filter(|n| !n.is_empty())
    {
        p.output = dev(n);
    }
    p.channel = cfg
        .get("input_channel")
        .and_then(|v| v.as_i64())
        .map(|c| c.clamp(-1, 63) as i32);
    p.exclusive = cfg.bool("exclusive_output", false);
    Some(p)
}

/// What the key lookup found, in quickrs.log (never the keys).
fn log_keys(state: &crate::keyfile::KeyState) {
    use crate::keyfile::KeyState;
    let msg = match state {
        KeyState::Found {
            path: Some(p),
            misnamed: true,
        } => format!(
            "keys: read from {} (saved as {}; keys.json is the right name, but it works)",
            p.display(),
            p.file_name().map(|n| n.to_string_lossy()).unwrap_or_default()
        ),
        KeyState::Found { path: Some(p), .. } => format!("keys: read from {}", p.display()),
        KeyState::Found { path: None, .. } => "keys: from the RSPLAY_*_KEY environment variables".into(),
        KeyState::Partial { path } => format!("keys: {} doesn't have both keys", path.display()),
        KeyState::Missing => "keys: none found (psarc songs need them; feedpaks don't)".into(),
        KeyState::Invalid { why, .. } => format!("keys: not read: {why}"),
    };
    crate::applog::write(&msg);
}

fn exe_dirs() -> Vec<PathBuf> {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        .into_iter()
        .collect()
}

/// The built-in lessons and those in the data folder's `lessons` (a file that won't read is logged and skipped).
/// None without the `lessons` feature (the shared build, D50): nothing then leads to a lesson.
fn load_lessons(data: &DataDir) -> Vec<Arc<Lesson>> {
    if !cfg!(feature = "lessons") {
        return Vec::new();
    }
    let (lessons, errors) = qrs_library::lessons::load(&data.lessons());
    for (file, why) in errors {
        crate::applog::write(&format!("lessons/{file}: not read: {why}"));
    }
    lessons.into_iter().map(Arc::new).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_python_device_settings_carry_over() {
        let dir = std::env::temp_dir().join(format!("qrs-state-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("config.json");
        std::fs::write(
            &path,
            r#"{"input_device": "NUX Audio (ASIO)", "input_channel": -1, "exclusive_output": true, "output_device": "Speakers (Realtek(R) Audio)"}"#,
        )
        .unwrap();
        let p = python_audio_prefs(&Config::load(&path)).unwrap();
        match &p.input {
            qrs_audio::Choice::Device(d) => {
                assert_eq!(d.backend, qrs_audio::backend::Backend::Asio);
                assert_eq!(d.name, "NUX Audio");
            }
            other => panic!("{other:?}"),
        }
        match &p.output {
            qrs_audio::Choice::Device(d) => assert_eq!(d.name, "Speakers (Realtek(R) Audio)"),
            other => panic!("{other:?}"),
        }
        assert_eq!(p.channel, Some(-1));
        assert!(p.exclusive);
        std::fs::write(&path, r#"{"theme": "midnight"}"#).unwrap();
        assert!(python_audio_prefs(&Config::load(&path)).is_none());
        let _ = std::fs::remove_dir_all(dir);
    }
}
