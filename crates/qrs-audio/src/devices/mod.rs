//! Plug and play: which devices to use, and keeping it right as devices come and go.
//!
//! `plan` turns the user's preferences (Auto / a device / Off, for input and output), the device ranking (`rank`)
//! and the imported Rocksmith setup (`rocksmith`) into an engine route. `Audio` runs it: it opens the engine on
//! that route, watches for devices being plugged in or out and for default-device changes (re-planning and
//! switching when the result changes), finds the guitar channel by itself the first time a string rings clearly
//! on one channel, and reports what happened.

pub mod rank;
pub mod rocksmith;

use std::path::PathBuf;
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, Sender, unbounded};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

use crate::backend::{self, Backend, BufferSize, DeviceEvent, DeviceInfo};
use crate::engine::{Engine, Event, InputRoute, Route};
use rank::{InputKind, RankedInput, rank_inputs, suggest_channel};
use rocksmith::RocksmithSetup;

/// A device the user picked (matched by id, then by name, so it's found again after a driver update).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DevicePref {
    pub backend: Backend,
    pub id: String,
    pub name: String,
}

impl DevicePref {
    pub fn of(d: &DeviceInfo) -> DevicePref {
        DevicePref {
            backend: d.id.backend,
            id: d.id.id.clone(),
            name: d.name.clone(),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "lowercase")]
pub enum Choice {
    /// Pick the best device by itself (and follow the system default for output).
    #[default]
    Auto,
    Device(DevicePref),
    /// No input / no sound.
    Off,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AudioPrefs {
    pub input: Choice,
    /// The guitar channel (0-based; -1 mixes all); None finds it by itself.
    pub channel: Option<i32>,
    pub output: Choice,
    /// WASAPI exclusive mode (lowest latency; other programs go quiet meanwhile).
    pub exclusive: bool,
    /// Buffer in frames (ASIO / exclusive WASAPI); None = the driver's or device's default.
    pub buffer: Option<u32>,
    /// Use the Rocksmith 2014 setup (RS_ASIO.ini, Rocksmith.ini) where nothing else is chosen.
    pub use_rocksmith: bool,
}

impl Default for AudioPrefs {
    fn default() -> Self {
        AudioPrefs {
            input: Choice::Auto,
            channel: None,
            output: Choice::Auto,
            exclusive: false,
            buffer: None,
            use_rocksmith: true,
        }
    }
}

/// What `plan` decided, and why.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Plan {
    pub route: Route,
    pub input: Option<RankedInput>,
    pub output: Option<DeviceInfo>,
    /// Plain-language notes ("Line (NUX Audio) isn't connected, using …").
    pub notes: Vec<String>,
    /// The channel came from the user or the Rocksmith setup (not auto-detected).
    pub channel_fixed: bool,
}

fn find<'a>(devices: &'a [DeviceInfo], p: &DevicePref, input: bool) -> Option<&'a DeviceInfo> {
    let ok = |d: &&DeviceInfo| {
        if input {
            d.input_channels > 0
        } else {
            d.output_channels > 0
        }
    };
    devices
        .iter()
        .filter(ok)
        .find(|d| d.id.backend == p.backend && d.id.id == p.id)
        .or_else(|| {
            devices
                .iter()
                .filter(ok)
                .find(|d| d.id.backend == p.backend && d.name == p.name)
        })
        .or_else(|| devices.iter().filter(ok).find(|d| d.name == p.name))
}

/// The route for these preferences and devices.
pub fn plan(prefs: &AudioPrefs, devices: &[DeviceInfo], rs: Option<&RocksmithSetup>) -> Plan {
    let rs = if prefs.use_rocksmith { rs } else { None };
    let ranked = rank_inputs(devices, rs);
    let mut notes = Vec::new();
    let best_auto = || {
        ranked
            .iter()
            .find(|r| r.score > 0 && !matches!(r.kind, InputKind::Virtual | InputKind::Test))
            .cloned()
    };
    let input: Option<RankedInput> = match &prefs.input {
        Choice::Off => None,
        Choice::Auto => best_auto(),
        Choice::Device(p) => match find(devices, p, true) {
            Some(d) => ranked.iter().find(|r| r.device.id == d.id).cloned(),
            None => {
                let alt = best_auto();
                notes.push(match &alt {
                    Some(a) => format!(
                        "{} isn't connected; using {} until it is",
                        p.name,
                        a.device.label()
                    ),
                    None => format!("{} isn't connected", p.name),
                });
                alt
            }
        },
    };
    let rs_asio = rs.and_then(|s| s.rs_asio.as_ref()).filter(|a| a.enable_asio);
    let input_is_rs_asio = input.as_ref().is_some_and(|i| {
        i.device.id.backend == Backend::Asio
            && rs_asio
                .and_then(|a| a.inputs.first())
                .is_some_and(|(drv, _)| drv.eq_ignore_ascii_case(&i.device.name))
    });

    // the guitar channel
    let (channel, channel_fixed) = match prefs.channel {
        Some(c) => (c, true),
        None if input_is_rs_asio => (
            rs_asio
                .and_then(|a| a.inputs.first())
                .map(|x| x.1 as i32)
                .unwrap_or(0),
            true,
        ),
        None => match input.as_ref() {
            Some(i) if i.device.input_channels == 1 => (0, true),
            Some(i) if i.kind == InputKind::RealToneCable => {
                (rs.map(|s| s.mono_channel as i32).unwrap_or(0), true)
            }
            _ => (-1, false), // listen to every channel until a string shows which one it is
        },
    };

    // the output
    let default_out = || {
        devices
            .iter()
            .find(|d| d.id.backend == Backend::Wasapi && d.is_default_output)
            .or_else(|| {
                devices
                    .iter()
                    .find(|d| d.id.backend == Backend::System && d.output_channels > 0)
            })
            .cloned()
    };
    let output: Option<DeviceInfo> = match &prefs.output {
        Choice::Off => None,
        Choice::Device(p) => match find(devices, p, false) {
            Some(d) => Some(d.clone()),
            None => {
                notes.push(format!("{} isn't connected; using the default output", p.name));
                default_out()
            }
        },
        Choice::Auto => {
            let asio_in = input.as_ref().filter(|i| i.device.id.backend == Backend::Asio);
            match (asio_in, rs_asio) {
                // RS_ASIO sends the game's sound to the same driver: duplex
                (Some(i), Some(a))
                    if input_is_rs_asio && a.output_driver.eq_ignore_ascii_case(&i.device.name) =>
                {
                    Some(i.device.clone())
                }
                _ => default_out(),
            }
        }
    };
    // only one ASIO driver can run: an ASIO output with a different ASIO input can't work
    let mut input = input;
    if let (Some(i), Some(o)) = (&input, &output)
        && i.device.id.backend == Backend::Asio
        && o.id.backend == Backend::Asio
        && i.device.id != o.id
    {
        notes.push(format!(
            "only one ASIO driver can run at a time: {} can't be used with {}",
            i.device.name, o.name
        ));
        input = None;
    }

    let buffer = match prefs.buffer {
        Some(n) => BufferSize::Frames(n),
        None => match rs_asio {
            Some(a) if input_is_rs_asio && a.buffer_mode == "custom" => {
                a.custom_buffer.map(BufferSize::Frames).unwrap_or_default()
            }
            _ => BufferSize::Default,
        },
    };
    let base_channel = match (&output, rs_asio) {
        (Some(o), Some(a))
            if o.id.backend == Backend::Asio && a.output_driver.eq_ignore_ascii_case(&o.name) =>
        {
            a.output_base_channel as usize
        }
        _ => 0,
    };
    let route = Route {
        input: input.as_ref().map(|i| InputRoute {
            device: i.device.id.clone(),
            channel,
        }),
        output: output.as_ref().map(|o| o.id.clone()),
        exclusive: prefs.exclusive,
        buffer,
        base_channel,
    };
    Plan {
        route,
        input,
        output,
        notes,
        channel_fixed,
    }
}

/// What the app may want to tell the user.
#[derive(Clone, Debug, PartialEq)]
pub enum AudioEvent {
    Engine(Event),
    /// The device list changed (plugged in / out).
    DevicesChanged,
    /// The route changed by itself (e.g. the guitar interface was plugged in): the new input's name.
    Switched {
        input: Option<String>,
        output: Option<String>,
    },
    /// The guitar channel was found from the signal.
    ChannelFound(i32),
}

struct State {
    prefs: AudioPrefs,
    library_dirs: Vec<PathBuf>,
    devices: Vec<DeviceInfo>,
    rocksmith: Option<RocksmithSetup>,
    plan: Plan,
    channel_locked: bool,
    /// Automatic lost the guitar interface it was using: wait for it rather than fall to a microphone.
    waiting_for: Option<RankedInput>,
}

fn guitar_like(k: InputKind) -> bool {
    matches!(k, InputKind::RealToneCable | InputKind::Interface)
}

/// On Automatic, when the guitar interface in use goes away, don't fall back to a microphone or the system
/// default (a webcam mic, heard through the speakers): keep no input and wait for the interface (or another
/// one) to come back. `prev` is the input in use until now.
fn hold_for_guitar(prev: Option<&RankedInput>, waiting: &mut Option<RankedInput>, p: &mut Plan, auto: bool) {
    if !auto {
        *waiting = None;
        return;
    }
    let new_ok = p.input.as_ref().is_some_and(|i| guitar_like(i.kind));
    if new_ok {
        *waiting = None;
        return;
    }
    if let Some(prev) = prev.filter(|i| guitar_like(i.kind)) {
        *waiting = Some(prev.clone());
    }
    if let Some(w) = waiting {
        p.route.input = None;
        p.input = None;
        p.channel_fixed = false;
        p.notes.push(format!(
            "{} isn't connected: waiting for it (or pick another input)",
            w.device.label()
        ));
    }
}

pub struct Audio {
    pub engine: Arc<Engine>,
    state: Arc<Mutex<State>>,
    tx: Sender<Msg>,
    events: Receiver<AudioEvent>,
    thread: Option<JoinHandle<()>>,
}

enum Msg {
    Prefs(AudioPrefs),
    Rescan,
    Stop,
}

/// Devices, refreshing ASIO only when no ASIO stream is running (loading another ASIO driver can disturb it).
fn scan(previous: &[DeviceInfo], asio_busy: bool) -> Vec<DeviceInfo> {
    let mut list = backend::enumerate_except(if asio_busy { Some(Backend::Asio) } else { None });
    if asio_busy {
        list.extend(previous.iter().filter(|d| d.id.backend == Backend::Asio).cloned());
    }
    list
}

impl Audio {
    /// Start on the best devices for these preferences.
    pub fn start(prefs: AudioPrefs, library_dirs: Vec<PathBuf>) -> Audio {
        let engine = Arc::new(Engine::new());
        let devices = scan(&[], false);
        let rocksmith = rocksmith::find_setup(&library_dirs);
        let plan = plan(&prefs, &devices, rocksmith.as_ref());
        engine.open(plan.route.clone());
        let state = Arc::new(Mutex::new(State {
            prefs,
            library_dirs,
            devices,
            rocksmith,
            plan,
            channel_locked: false,
            waiting_for: None,
        }));
        let (tx, rx) = unbounded();
        let (ev_tx, ev_rx) = unbounded();
        let (e2, s2) = (engine.clone(), state.clone());
        let thread = std::thread::Builder::new()
            .name("qrs-devices".into())
            .spawn(move || manager(e2, s2, rx, ev_tx))
            .expect("spawn device manager");
        Audio {
            engine,
            state,
            tx,
            events: ev_rx,
            thread: Some(thread),
        }
    }

    pub fn prefs(&self) -> AudioPrefs {
        self.state.lock().prefs.clone()
    }

    /// New preferences: re-plan and reopen if the route changes.
    pub fn set_prefs(&self, p: AudioPrefs) {
        let _ = self.tx.send(Msg::Prefs(p));
    }

    pub fn rescan(&self) {
        let _ = self.tx.send(Msg::Rescan);
    }

    pub fn devices(&self) -> Vec<DeviceInfo> {
        self.state.lock().devices.clone()
    }

    pub fn ranked_inputs(&self) -> Vec<RankedInput> {
        let s = self.state.lock();
        rank_inputs(&s.devices, s.rocksmith.as_ref())
    }

    pub fn outputs(&self) -> Vec<DeviceInfo> {
        self.state
            .lock()
            .devices
            .iter()
            .filter(|d| d.output_channels > 0)
            .cloned()
            .collect()
    }

    pub fn plan(&self) -> Plan {
        self.state.lock().plan.clone()
    }

    pub fn rocksmith(&self) -> Option<RocksmithSetup> {
        self.state.lock().rocksmith.clone()
    }

    pub fn events(&self) -> Vec<AudioEvent> {
        self.events.try_iter().collect()
    }
}

impl Drop for Audio {
    fn drop(&mut self) {
        let _ = self.tx.send(Msg::Stop);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

fn apply(engine: &Engine, s: &mut State, events: &Sender<AudioEvent>, announce: bool) {
    let mut p = plan(&s.prefs, &s.devices, s.rocksmith.as_ref());
    let prev = s.plan.input.clone();
    hold_for_guitar(
        prev.as_ref(),
        &mut s.waiting_for,
        &mut p,
        s.prefs.input == Choice::Auto,
    );
    // keep a channel found earlier on the same input
    if !p.channel_fixed
        && s.channel_locked
        && let (Some(new), Some(old)) = (p.route.input.as_mut(), s.plan.route.input.as_ref())
        && new.device == old.device
    {
        new.channel = old.channel;
    }
    if p.route != s.plan.route {
        if p.route.input.as_ref().map(|i| &i.device) != s.plan.route.input.as_ref().map(|i| &i.device) {
            s.channel_locked = false;
        }
        if same_but_channel(&p.route, &s.plan.route) {
            // only the guitar channel changed: no need to reopen anything
            engine.set_channel(p.route.input.as_ref().map(|i| i.channel).unwrap_or(0));
            p.channel_fixed |= s.channel_locked;
            s.plan = p;
            return;
        }
        engine.open(p.route.clone());
        if announce {
            let _ = events.send(AudioEvent::Switched {
                input: p.input.as_ref().map(|i| i.device.label()),
                output: p.output.as_ref().map(|o| o.label()),
            });
        }
    }
    p.channel_fixed |= s.channel_locked;
    s.plan = p;
}

fn manager(engine: Arc<Engine>, state: Arc<Mutex<State>>, rx: Receiver<Msg>, events: Sender<AudioEvent>) {
    #[cfg(windows)]
    crate::backend::wasapi::com_init();
    let (dev_tx, dev_rx) = unbounded::<DeviceEvent>();
    #[cfg(windows)]
    let _watcher = crate::backend::wasapi::watch(dev_tx.clone());
    let _ = &dev_tx;
    let mut pending_since: Option<Instant> = None;
    let mut last_poll = Instant::now();
    let mut asio_names = backend::asio_driver_names();
    let mut channel_votes: (i32, u32) = (i32::MIN, 0);
    loop {
        match rx.recv_timeout(Duration::from_millis(100)) {
            Ok(Msg::Stop) => return,
            Ok(Msg::Prefs(p)) => {
                let mut s = state.lock();
                s.prefs = p;
                s.waiting_for = None; // a choice you just made: take what's there now
                apply(&engine, &mut s, &events, false);
            }
            Ok(Msg::Rescan) => {
                let mut s = state.lock();
                let busy = route_uses_asio(&s.plan.route);
                s.devices = scan(&s.devices, busy);
                s.rocksmith = rocksmith::find_setup(&s.library_dirs);
                apply(&engine, &mut s, &events, false);
                let _ = events.send(AudioEvent::DevicesChanged);
            }
            Err(crossbeam_channel::RecvTimeoutError::Timeout) => {}
            Err(_) => return,
        }
        for ev in engine.events() {
            let _ = events.send(AudioEvent::Engine(ev));
        }
        // device notifications: wait for them to settle (a USB device brings several endpoints at once)
        let mut got = false;
        while dev_rx.try_recv().is_ok() {
            got = true;
        }
        if got {
            pending_since = Some(Instant::now());
        }
        // ASIO doesn't notify: look at the installed drivers now and then (in the registry: listing the devices
        // would load every driver). An interface's own endpoints come and go with WASAPI notifications above.
        if last_poll.elapsed() > Duration::from_secs(5) {
            last_poll = Instant::now();
            let names = backend::asio_driver_names();
            if names != asio_names {
                asio_names = names;
                pending_since = Some(Instant::now());
            }
        }
        if pending_since.is_some_and(|t| t.elapsed() > Duration::from_millis(600)) {
            pending_since = None;
            let mut s = state.lock();
            let busy = route_uses_asio(&s.plan.route);
            s.devices = scan(&s.devices, busy);
            apply(&engine, &mut s, &events, true);
            let _ = events.send(AudioEvent::DevicesChanged);
        }
        // the guitar channel, from the signal
        let mut s = state.lock();
        if !s.plan.channel_fixed && !s.channel_locked && s.plan.route.input.is_some() {
            let m = engine.meters();
            let n = m.channel_count.min(m.channels.len());
            if n > 1 {
                match suggest_channel(&m.channels[..n]) {
                    Some(c) if c == channel_votes.0 => channel_votes.1 += 1,
                    Some(c) => channel_votes = (c, 1),
                    None => {}
                }
                // ~1 s of a clear signal on the same channel
                if channel_votes.1 >= 10 {
                    let c = channel_votes.0;
                    channel_votes = (i32::MIN, 0);
                    s.channel_locked = true;
                    s.plan.channel_fixed = true;
                    if let Some(i) = s.plan.route.input.as_mut() {
                        i.channel = c;
                    }
                    engine.set_channel(c);
                    let _ = events.send(AudioEvent::ChannelFound(c));
                }
            }
        }
    }
}

/// The same devices and settings, only another input channel.
fn same_but_channel(a: &Route, b: &Route) -> bool {
    match (&a.input, &b.input) {
        (Some(x), Some(y)) if x.device == y.device => {
            let mut a = a.clone();
            if let Some(i) = a.input.as_mut() {
                i.channel = y.channel;
            }
            a == *b
        }
        _ => false,
    }
}

fn route_uses_asio(r: &Route) -> bool {
    r.input
        .as_ref()
        .is_some_and(|i| i.device.backend == Backend::Asio)
        || r.output.as_ref().is_some_and(|o| o.backend == Backend::Asio)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::{Bus, DeviceId, Hardware};

    fn dev(backend: Backend, name: &str, ins: u16, outs: u16, default_out: bool, bus: Bus) -> DeviceInfo {
        DeviceInfo {
            id: DeviceId {
                backend,
                id: format!("id:{name}"),
            },
            name: name.into(),
            input_channels: ins,
            output_channels: outs,
            input_channel_names: vec![],
            default_rate: 48000,
            is_default_input: false,
            is_default_output: default_out,
            hardware: Hardware {
                bus,
                adapter_name: name.into(),
                ..Default::default()
            },
        }
    }

    fn owner() -> Vec<DeviceInfo> {
        vec![
            dev(Backend::Wasapi, "Line (NUX Audio)", 2, 0, false, Bus::Usb),
            dev(
                Backend::Wasapi,
                "Microphone (Anker PowerConf C200)",
                2,
                0,
                false,
                Bus::Usb,
            ),
            dev(
                Backend::Wasapi,
                "Speakers (Realtek(R) Audio)",
                0,
                2,
                true,
                Bus::Pci,
            ),
            dev(Backend::Wasapi, "Speakers (NUX Audio)", 0, 2, false, Bus::Usb),
            dev(Backend::Asio, "NUX Audio", 2, 2, false, Bus::Unknown),
        ]
    }

    fn rs_asio() -> RocksmithSetup {
        RocksmithSetup {
            exclusive_mode: true,
            rs_asio: Some(rocksmith::RsAsio {
                enable_asio: true,
                enable_wasapi_outputs: true,
                buffer_mode: "custom".into(),
                custom_buffer: Some(64),
                inputs: vec![("NUX Audio".into(), 0)],
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    #[test]
    fn owner_setup_plans_like_rocksmith() {
        let p = plan(&AudioPrefs::default(), &owner(), Some(&rs_asio()));
        let i = p.route.input.unwrap();
        assert_eq!(i.device.backend, Backend::Asio);
        assert_eq!(i.channel, 0);
        assert_eq!(p.output.unwrap().name, "Speakers (Realtek(R) Audio)");
        assert_eq!(p.route.buffer, BufferSize::Frames(64));
    }

    #[test]
    fn without_rocksmith_the_channel_is_found_later() {
        let p = plan(&AudioPrefs::default(), &owner(), None);
        assert_eq!(p.route.input.unwrap().channel, -1);
        assert!(!p.channel_fixed);
    }

    #[test]
    fn unplugged_choice_falls_back_and_says_so() {
        let prefs = AudioPrefs {
            input: Choice::Device(DevicePref {
                backend: Backend::Wasapi,
                id: "gone".into(),
                name: "Focusrite USB".into(),
            }),
            ..Default::default()
        };
        let p = plan(&prefs, &owner(), None);
        assert!(p.notes[0].contains("isn't connected"));
        assert!(p.route.input.is_some());
    }

    #[test]
    fn duplex_when_rs_asio_outputs_to_the_same_driver() {
        let mut rs = rs_asio();
        rs.rs_asio.as_mut().unwrap().output_driver = "NUX Audio".into();
        let p = plan(&AudioPrefs::default(), &owner(), Some(&rs));
        assert_eq!(p.route.output, Some(p.route.input.unwrap().device));
    }

    #[test]
    fn off_means_off() {
        let prefs = AudioPrefs {
            input: Choice::Off,
            output: Choice::Off,
            ..Default::default()
        };
        let p = plan(&prefs, &owner(), None);
        assert!(p.route.input.is_none() && p.route.output.is_none());
    }

    #[test]
    fn a_channel_change_is_not_a_new_route() {
        let a = plan(&AudioPrefs::default(), &owner(), None).route;
        let mut b = a.clone();
        b.input.as_mut().unwrap().channel = 1;
        assert!(same_but_channel(&b, &a));
        let mut c = b.clone();
        c.exclusive = true;
        assert!(!same_but_channel(&c, &a));
        assert!(!same_but_channel(&Route::default(), &a));
    }

    #[test]
    fn automatic_waits_for_the_interface_instead_of_a_microphone() {
        let all = owner();
        let before = plan(&AudioPrefs::default(), &all, None);
        assert_eq!(before.input.as_ref().unwrap().kind, InputKind::Interface);
        // the NUX goes (both its endpoints): Automatic would take the webcam mic
        let left: Vec<DeviceInfo> = all.into_iter().filter(|d| !d.name.contains("NUX")).collect();
        let mut p = plan(&AudioPrefs::default(), &left, None);
        assert_eq!(p.input.as_ref().unwrap().kind, InputKind::Microphone);
        let mut waiting = None;
        hold_for_guitar(before.input.as_ref(), &mut waiting, &mut p, true);
        assert!(p.route.input.is_none() && waiting.is_some());
        assert!(p.notes[0].contains("waiting"), "{:?}", p.notes);
        // still waiting on the next re-plan (the previous plan had no input)
        let mut p2 = plan(&AudioPrefs::default(), &left, None);
        hold_for_guitar(None, &mut waiting, &mut p2, true);
        assert!(p2.route.input.is_none());
        // it's back: taken again
        let mut p3 = plan(&AudioPrefs::default(), &owner(), None);
        hold_for_guitar(None, &mut waiting, &mut p3, true);
        assert!(p3.route.input.is_some() && waiting.is_none());
        // a microphone chosen at startup (nothing better ever seen) is used as it is
        let mut p4 = plan(&AudioPrefs::default(), &left, None);
        let mut w4 = None;
        hold_for_guitar(None, &mut w4, &mut p4, true);
        assert!(p4.route.input.is_some());
    }

    #[test]
    fn prefs_roundtrip_json() {
        let prefs = AudioPrefs {
            input: Choice::Device(DevicePref {
                backend: Backend::Asio,
                id: "NUX Audio".into(),
                name: "NUX Audio".into(),
            }),
            channel: Some(1),
            ..Default::default()
        };
        let j = serde_json::to_string(&prefs).unwrap();
        assert!(j.contains("\"mode\":\"device\""));
        let back: AudioPrefs = serde_json::from_str(&j).unwrap();
        assert_eq!(back, prefs);
        let partial: AudioPrefs = serde_json::from_str("{\"exclusive\": true}").unwrap();
        assert!(partial.exclusive && partial.input == Choice::Auto);
    }
}
