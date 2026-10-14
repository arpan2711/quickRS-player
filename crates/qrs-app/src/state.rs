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
use qrs_library::{Config, DataDir, Group, Library, Prefer, SongEntry, UserData, config, group};
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
    /// The NUX MG-300 over USB MIDI, started the first time something shows it (the MG-300 tile); a simulated
    /// pedal in silent mode, so screenshots and night tests never touch the real one.
    #[cfg(feature = "nux")]
    pedal: Option<qrs_mg300::Pedal>,
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
        let mut config = Config::load(data.config());
        // the Python app's sound devices, as quickRS's choice (once)
        if config.get("audio").is_none()
            && let Some(prefs) = python_audio_prefs(&config)
        {
            config.set("audio", serde_json::to_value(&prefs).unwrap_or_default());
            let _ = config.save();
        }
        let userdata = UserData::load(data.userdata());
        let keys_file = data.keys();
        let keys = qrs_formats::load_keys(keys_file.is_file().then_some(keys_file.as_path()), &exe_dirs())
            .unwrap_or_default();
        theme::set_theme(ThemeName::parse(&config.str("theme", "midnight")));
        qrs_ui::knobs::set_style(qrs_ui::knobs::KnobStyle::parse(&config.str("knobs", "amp")));
        theme::set_string_colors(config.bool("string_colors", false));
        let mut dirs = config.library_dirs();
        for d in dirs.clone() {
            if let Some(f) = config::sibling_feedpak_dir(&d)
                && !dirs.contains(&f)
            {
                dirs.push(f);
            }
        }
        let library = Library::new();
        library.start_scan(keys.clone(), dirs.clone(), data.index());
        let audio = (audio != AudioMode::Off).then(|| {
            let prefs: AudioPrefs = if audio == AudioMode::Silent {
                AudioPrefs {
                    input: qrs_audio::Choice::Off,
                    output: qrs_audio::Choice::Off,
                    use_rocksmith: false,
                    ..Default::default()
                }
            } else if audio == AudioMode::Shots {
                use qrs_audio::backend::{Backend, null};
                null::enable_stand_in();
                let stand_in = qrs_audio::DevicePref {
                    backend: Backend::Null,
                    id: null::STAND_IN.into(),
                    name: null::STAND_IN_NAME.into(),
                };
                AudioPrefs {
                    input: qrs_audio::Choice::Device(stand_in.clone()),
                    channel: Some(0),
                    output: qrs_audio::Choice::Device(stand_in),
                    use_rocksmith: false,
                    ..Default::default()
                }
            } else {
                config
                    .get("audio")
                    .and_then(|v| serde_json::from_value(v.clone()).ok())
                    .unwrap_or_default()
            };
            let a = Audio::start(prefs, dirs.clone());
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
        AppState {
            ctx: ctx.clone(),
            data,
            config,
            userdata,
            keys,
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
            toast: migrated.map(|r| {
                (
                    format!("Brought over {} from the Python app", r.summary()),
                    Instant::now(),
                )
            }),
            #[cfg(feature = "nux")]
            pedal: None,
        }
    }

    #[cfg(feature = "nux")]
    pub fn pedal(&mut self) -> &qrs_mg300::Pedal {
        let ctx = self.ctx.clone();
        let silent = self.audio_mode != AudioMode::Devices;
        let shots = self.audio_mode == AudioMode::Shots;
        self.pedal.get_or_insert_with(|| {
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
        })
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

    /// The guitar input is a microphone (an acoustic guitar played to a webcam or laptop mic, Rocksmith's
    /// "microphone" mode, PLAN.md section 4.1).
    pub fn mic_input(&self) -> bool {
        self.audio
            .as_ref()
            .and_then(|a| a.plan().input)
            .is_some_and(|i| i.kind == qrs_audio::devices::rank::InputKind::Microphone)
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
            a.engine.set_onset_floor(if mic { 0.012 } else { 0.004 });
        }
        if mic && !first {
            self.say(
                "A microphone is the guitar input: hearing it through the speakers is off (M turns it on)",
            );
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

    /// A folder of songs added to the library for good (dropped on the window): saved in config.json's
    /// library_dirs with the folders read so far (the ones found by themselves too), and read now.
    pub fn add_folder(&mut self, dir: &std::path::Path) -> Result<String, String> {
        let dir = dir.to_path_buf();
        if self.dirs.contains(&dir) {
            return Ok(format!("{} is already in the library", dir.display()));
        }
        let mut saved = self.config.library_dirs();
        saved.retain(|d| d != &dir);
        saved.push(dir.clone());
        self.config.set_library_dirs(&saved);
        self.config
            .save()
            .map_err(|e| format!("couldn't save config.json: {e}"))?;
        self.dirs.push(dir.clone());
        if let Some(f) = config::sibling_feedpak_dir(&dir)
            && !self.dirs.contains(&f)
        {
            self.dirs.push(f);
        }
        self.library
            .start_scan(self.keys.clone(), self.dirs.clone(), self.data.index());
        Ok(format!(
            "Added {} to the library: reading its songs",
            dir.display()
        ))
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

    /// A message that's also an error worth keeping (quickrs.log).
    pub fn say_error(&mut self, msg: impl Into<String>) {
        let m = msg.into();
        crate::applog::write(&m);
        self.say(m);
    }

    pub fn save_config(&self) {
        let _ = self.config.save();
    }

    /// The pick in the sidebar: a dark theme <-> Daylight (remembered in config.json).
    pub fn toggle_theme(&mut self) {
        let next = if theme::is_dark() {
            ThemeName::Daylight
        } else {
            ThemeName::parse(&self.config.str("dark_theme", "midnight"))
        };
        self.apply_theme(next);
    }

    pub fn apply_theme(&mut self, name: ThemeName) {
        theme::set_theme(name);
        self.config.set("theme", name.key());
        if theme::is_dark() {
            self.config.set("dark_theme", name.key());
        }
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

fn exe_dirs() -> Vec<PathBuf> {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        .into_iter()
        .collect()
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
