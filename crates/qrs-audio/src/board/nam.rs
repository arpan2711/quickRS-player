//! NAM captures and IR cabs (new in quickRS, P31, D80): an amp that is a Neural Amp Modeler capture (a `.nam` file
//! run by nam-rs) and a cab that is an impulse response (a WAV), as two more models on the board (`amp_nam`,
//! `cab_ir`) beside our own.
//!
//! The files live in the app's captures library; this module only knows them by id. `register_*` parses a file and
//! keeps it in a process-wide registry (off the audio thread: it allocates and takes a lock); a block names its file
//! by `Block::asset`, and `make_pedal` builds a fresh runnable model from the registry for each pedal, so nothing is
//! loaded on the audio thread. A block whose file isn't registered (deleted, or another computer's) passes the
//! signal through.
//!
//! - Level: a capture's input is scaled by its calibration (`input_level_dbu`: our input's hard strums peak near
//!   -12 dBFS, taken as an interface where 0 dBFS = +12 dBu, NAM's default, or the reference level set in Settings ›
//!   Neural Amp Modeler: `set_input_dbu`), its output by its loudness (to
//!   `LOUDNESS_TARGET`, as the NAM plugin normalises), then the knobs. A DC blocker follows, as in the plugin.
//! - Rate: a capture runs at its own rate (48 kHz mostly). At another device rate the signal is resampled to it and
//!   back (rubato, fixed chunks through FIFOs; adds `ResampledModel::latency` samples).
//! - IR: resampled to the device rate when the pedal is made, cut to `IR_MAX_S`, normalised to unit energy; run by a
//!   zero-latency convolver (the first `IR_BLOCK` taps directly, the rest by uniformly partitioned FFTs).
//! - Controls that are real (P37.2, AUDIT B3): a NAM amp has a post-capture Bass / Mid / Treble (0 dB = as captured),
//!   a Gate in front of it and a Gain that steps through the unit's captures (the "gain ladder": the library tells
//!   `set_ladder` which captures are settings of one unit; the board crossfades when it changes); a NAM pedal has a
//!   Blend with the dry input (and "Clean low") and the same Gain. An IR cab has a DI blend.
//! - `generation` counts registrations, so a board built before the library opened can rebuild its file blocks
//!   (AUDIT B11: a saved NAM rig played dry at start-up).

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, OnceLock, RwLock};

use nam_rs::{Model, NamModel};
use realfft::num_complex::Complex32;
use realfft::{ComplexToReal, RealFftPlanner, RealToComplex};
use rubato::audioadapter_buffers::direct::InterleavedSlice;
use rubato::{Fft, FixedSync, Resampler};

use super::dsp::{Biquad, Filter};
use super::{MAX_KNOBS, Pedal, dsp};

/// Where a capture's output is brought to (dB, the loudness the file states). At NAM's own -18, six real TONE3000
/// Fender captures came out -16.9 to -22.4 dB RMS on our riff (`quickrs-cli nam level`), ~2.5 dB over our amps with
/// a cab (-20.7 to -25.4); -21 matches them, so MODELLED / NAM compares at one level. (NAM's tiny example model came
/// out 10 dB under; it's a bass DI, not a guide.)
pub const LOUDNESS_TARGET: f32 = -21.0;
/// A pedal capture is brought to the same loudness: about the dry guitar's, as a real drive at unity, so its Output
/// knob is how hard it pushes the amp (as captured, the Strokes' Jekyll & Hyde came out ~13 dB over and clipped the
/// amp after it).
pub const PEDAL_TARGET: f32 = -18.0;
/// The interface level our input is taken as by default (dBu at 0 dBFS) for a capture's input calibration: NAM's
/// own default, right for most pro interfaces (P37.15: Settings › Neural Amp Modeler can change it, `set_input_dbu`).
pub const INPUT_DBU: f32 = 12.0;
/// The range the setting is held to (dBu at 0 dBFS).
pub const INPUT_DBU_RANGE: (f32, f32) = (-20.0, 30.0);

/// The interface reference level in force (f32 bits; 0 = not set, `INPUT_DBU`).
static INPUT_REF: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

/// The interface reference level captures are calibrated against (dBu at 0 dBFS).
pub fn input_dbu() -> f32 {
    match INPUT_REF.load(Ordering::Acquire) {
        0 => INPUT_DBU,
        b => f32::from_bits(b),
    }
}

/// Set the interface reference level (clamped to `INPUT_DBU_RANGE`); running boards rebuild their capture blocks
/// (the calibration is fixed when a block is made).
pub fn set_input_dbu(dbu: f32) {
    let v = clamp_dbu(dbu);
    if (v - input_dbu()).abs() > 1e-4 {
        INPUT_REF.store(
            if v == 0.0 {
                (-0.0f32).to_bits()
            } else {
                v.to_bits()
            },
            Ordering::Release,
        );
        bump();
    }
}

/// A reference level as kept: within `INPUT_DBU_RANGE`, the default for nonsense.
pub fn clamp_dbu(dbu: f32) -> f32 {
    if dbu.is_finite() {
        dbu.clamp(INPUT_DBU_RANGE.0, INPUT_DBU_RANGE.1)
    } else {
        INPUT_DBU
    }
}

/// A capture's input gain (linear): an interface where 0 dBFS = `reference` dBu, into a capture trained at
/// `capture_dbu` dBu at its 0 dBFS (the file's `input_level_dbu`; none said: as is). A higher reference drives the
/// capture harder (the same digital level stands for a hotter signal).
pub fn input_gain(reference: f32, capture_dbu: Option<f32>) -> f32 {
    capture_dbu.map(|l| db(reference - l)).unwrap_or(1.0)
}
/// An IR is cut to this long (s).
pub const IR_MAX_S: f32 = 0.5;
/// The convolver's block (samples): the taps run directly, then FFT partitions of this size.
const IR_BLOCK: usize = 128;

/// What a capture file is, from its metadata.
#[derive(Clone, Debug, PartialEq)]
pub struct CaptureInfo {
    pub rate: f64,
    pub name: Option<String>,
    pub modeled_by: Option<String>,
    pub gear: Option<String>,
    /// Whether the capture has a speaker cab in it (so a cab after it would be a second one); None if it doesn't say.
    pub includes_cab: Option<bool>,
    pub loudness: Option<f32>,
    pub input_dbu: Option<f32>,
    pub architecture: String,
}

enum Loaded {
    Nam { model: Arc<NamModel>, info: CaptureInfo },
    Ir { samples: Arc<Vec<f32>>, rate: u32 },
}

/// A file's level trim (dB), measured against the model of ours it stands in for (P33: the app's library keeps it):
/// on top of the calibration, so a capture plays as loud as our model at the same gain.
fn trims() -> &'static RwLock<HashMap<u32, f32>> {
    static T: OnceLock<RwLock<HashMap<u32, f32>>> = OnceLock::new();
    T.get_or_init(|| RwLock::new(HashMap::new()))
}

pub fn set_trim(id: u32, db: f32) {
    let old = trims().write().unwrap_or_else(|e| e.into_inner()).insert(id, db);
    if old != Some(db) {
        bump();
    }
}

fn trim(id: u32) -> f32 {
    db(trims()
        .read()
        .unwrap_or_else(|e| e.into_inner())
        .get(&id)
        .copied()
        .unwrap_or(0.0))
}

static GENERATION: AtomicU64 = AtomicU64::new(0);

/// How many times the registry or the trims changed: a running board whose file blocks were built at another
/// generation rebuilds them (`Engine::refresh_files`).
pub fn generation() -> u64 {
    GENERATION.load(Ordering::Acquire)
}

fn bump() {
    GENERATION.fetch_add(1, Ordering::AcqRel);
}

/// The gain ladder: each capture's unit (an id the library gives: one TONE3000 tone, one kind) and its gain.
fn ladders() -> &'static RwLock<HashMap<u32, (u64, f32)>> {
    static L: OnceLock<RwLock<HashMap<u32, (u64, f32)>>> = OnceLock::new();
    L.get_or_init(|| RwLock::new(HashMap::new()))
}

/// Capture `asset` is the setting `gain` (0-10) of unit `unit` (P37.2).
pub fn set_ladder(asset: u32, unit: u64, gain: f32) {
    ladders()
        .write()
        .unwrap_or_else(|e| e.into_inner())
        .insert(asset, (unit, gain));
}

/// A capture's own gain, when the library said.
pub fn ladder_gain(asset: u32) -> Option<f32> {
    ladders()
        .read()
        .unwrap_or_else(|e| e.into_inner())
        .get(&asset)
        .map(|l| l.1)
}

/// The captures of `asset`'s unit that are loaded, by gain (just `asset` when it has no ladder).
pub fn ladder(asset: u32) -> Vec<(u32, f32)> {
    let l = ladders().read().unwrap_or_else(|e| e.into_inner());
    let Some(&(unit, g)) = l.get(&asset) else {
        return vec![(asset, 0.0)];
    };
    let mut v: Vec<(u32, f32)> = l
        .iter()
        .filter(|(id, (u, _))| *u == unit && (**id == asset || is_registered(**id)))
        .map(|(id, (_, g))| (*id, *g))
        .collect();
    if v.is_empty() {
        v.push((asset, g));
    }
    v.sort_by(|a, b| a.1.total_cmp(&b.1).then(a.0.cmp(&b.0)));
    v
}

/// Which capture plays for a block on `asset` with its Gain at `gain`: below 0, `asset` itself; else the unit's
/// capture nearest that gain (the lower id on a tie).
pub fn ladder_pick(asset: u32, gain: f32) -> u32 {
    if gain < 0.0 || asset == 0 {
        return asset;
    }
    ladder(asset)
        .into_iter()
        .min_by(|a, b| {
            (a.1 - gain)
                .abs()
                .total_cmp(&(b.1 - gain).abs())
                .then(a.0.cmp(&b.0))
        })
        .map(|a| a.0)
        .unwrap_or(asset)
}

/// A capture Gain knob's value snapped to its unit's steps (P37.6c): below 0 (or with one capture, nothing to step
/// to) "as captured" (-1), else the nearest step's own gain.
pub fn ladder_snap(asset: u32, v: f32) -> f32 {
    let steps = ladder(asset);
    if v < 0.0 || asset == 0 || steps.len() <= 1 {
        return -1.0;
    }
    steps
        .iter()
        .map(|s| s.1)
        .min_by(|a, b| (a - v).abs().total_cmp(&(b - v).abs()))
        .unwrap_or(-1.0)
}

fn registry() -> &'static RwLock<HashMap<u32, Loaded>> {
    static R: OnceLock<RwLock<HashMap<u32, Loaded>>> = OnceLock::new();
    R.get_or_init(|| RwLock::new(HashMap::new()))
}

/// Read a capture's metadata without keeping it.
pub fn inspect(json: &str) -> Result<CaptureInfo, String> {
    let m = NamModel::from_json_str(json).map_err(|e| format!("not a NAM capture: {e}"))?;
    // (a model that can't run here is refused now, not when it's put on a board)
    Model::from_nam(&m).map_err(|e| format!("this capture can't be run: {e}"))?;
    Ok(info_of(&m))
}

fn info_of(m: &NamModel) -> CaptureInfo {
    let md = m.metadata_typed();
    let gear = match (md.gear_make.as_deref(), md.gear_model.as_deref()) {
        (Some(a), Some(b)) => Some(format!("{a} {b}")),
        (Some(a), None) | (None, Some(a)) => Some(a.to_string()),
        _ => None,
    };
    CaptureInfo {
        rate: m.expected_sample_rate(),
        name: md.name.clone(),
        modeled_by: md.modeled_by.clone(),
        gear,
        includes_cab: m.includes_cab(),
        loudness: m.loudness(),
        input_dbu: m.input_level_dbu(),
        architecture: m.architecture.clone(),
    }
}

/// Parse a `.nam` file and keep it under `id` (replacing what was there).
pub fn register_nam(id: u32, json: &str) -> Result<CaptureInfo, String> {
    let m = NamModel::from_json_str(json).map_err(|e| format!("not a NAM capture: {e}"))?;
    Model::from_nam(&m).map_err(|e| format!("this capture can't be run: {e}"))?;
    let info = info_of(&m);
    let mut r = registry().write().unwrap_or_else(|e| e.into_inner());
    r.insert(
        id,
        Loaded::Nam {
            model: Arc::new(m),
            info: info.clone(),
        },
    );
    drop(r);
    bump();
    Ok(info)
}

/// Decode an IR (WAV) and keep it under `id`: its length in seconds.
pub fn register_ir(id: u32, wav: &[u8]) -> Result<f32, String> {
    let pcm = crate::decode::decode_symphonia(wav, "wav")
        .map_err(|e| format!("not a WAV impulse response: {e}"))?;
    let ch = pcm.channels.max(1) as usize;
    // (a stereo IR: its left channel)
    let mono: Vec<f32> = pcm.samples.iter().step_by(ch).copied().collect();
    if mono.is_empty() || mono.iter().all(|&x| x == 0.0) {
        return Err("the impulse response is empty".into());
    }
    let secs = mono.len() as f32 / pcm.rate.max(1) as f32;
    let mut r = registry().write().unwrap_or_else(|e| e.into_inner());
    r.insert(
        id,
        Loaded::Ir {
            samples: Arc::new(mono),
            rate: pcm.rate,
        },
    );
    drop(r);
    bump();
    Ok(secs)
}

pub fn unregister(id: u32) {
    registry().write().unwrap_or_else(|e| e.into_inner()).remove(&id);
    bump();
}

pub fn is_registered(id: u32) -> bool {
    registry()
        .read()
        .unwrap_or_else(|e| e.into_inner())
        .contains_key(&id)
}

/// A registered capture's metadata.
pub fn capture_info(id: u32) -> Option<CaptureInfo> {
    match registry().read().unwrap_or_else(|e| e.into_inner()).get(&id) {
        Some(Loaded::Nam { info, .. }) => Some(info.clone()),
        _ => None,
    }
}

fn db(x: f32) -> f32 {
    10f32.powf(x / 20.0)
}

// -- the editor's picture ------------------------------------------------------------------------------------------

/// What a file does, for the editor's graph (measured once per file, kept): a capture's output against its input
/// over one cycle of a low note at a hard strum's level (how it breaks up); an IR's frequency response.
pub fn picture(asset: u32) -> Option<super::picture::Picture> {
    use super::picture::Picture;
    static CACHE: OnceLock<std::sync::Mutex<HashMap<u32, Picture>>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| std::sync::Mutex::new(HashMap::new()));
    if let Some(p) = cache.lock().ok()?.get(&asset) {
        return Some(p.clone());
    }
    let what = match registry().read().unwrap_or_else(|e| e.into_inner()).get(&asset) {
        Some(Loaded::Nam { .. }) => true,
        Some(Loaded::Ir { .. }) => false,
        None => return None,
    };
    let rate = 48000u32;
    let pic = if what {
        // a 110 Hz note peaking at -12 dBFS (where calibration puts a hard strum), after 0.2 s to settle
        let mut amp = make_amp(asset, rate as f32);
        let per = (rate as f32 / 110.0) as usize;
        let n = rate as usize / 5 + per;
        let x: Vec<f32> = (0..n)
            .map(|i| 0.25 * (2.0 * std::f32::consts::PI * 110.0 * i as f32 / rate as f32).sin())
            .collect();
        let mut y = x.clone();
        amp.process(&mut y);
        let (xs, ys) = (&x[n - per..], &y[n - per..]);
        // (a capture has memory: its output loops around the input; the mean output at each input level is the curve)
        const BINS: usize = 41;
        let mut sum = [0.0f32; BINS];
        let mut cnt = [0u32; BINS];
        for (&a, &b) in xs.iter().zip(ys) {
            let k = (((a / 0.25 + 1.0) / 2.0 * (BINS - 1) as f32).round() as usize).min(BINS - 1);
            sum[k] += b;
            cnt[k] += 1;
        }
        let pts: Vec<(f32, f32)> = (0..BINS)
            .filter(|&k| cnt[k] > 0)
            .map(|k| (k as f32 / (BINS - 1) as f32 * 2.0 - 1.0, sum[k] / cnt[k] as f32))
            .collect();
        let top = pts.iter().fold(1e-6f32, |a, p| a.max(p.1.abs()));
        Picture::Transfer(pts.into_iter().map(|(a, b)| (a, b / top)).collect())
    } else {
        let ir = match registry().read().unwrap_or_else(|e| e.into_inner()).get(&asset) {
            Some(Loaded::Ir { samples, rate: r }) => prepare_ir(samples, *r, rate),
            _ => return None,
        };
        let len = ir.len().next_power_of_two().max(4096);
        let mut planner = RealFftPlanner::<f32>::new();
        let fft = planner.plan_fft_forward(len);
        let mut buf = fft.make_input_vec();
        buf[..ir.len()].copy_from_slice(&ir);
        let mut spec = fft.make_output_vec();
        fft.process(&mut buf, &mut spec).ok()?;
        let bin = rate as f32 / len as f32;
        let mag = |f: f32| {
            // a sixth of an octave around f, averaged (power)
            let (lo, hi) = (
                (f / 1.06 / bin) as usize,
                ((f * 1.06 / bin) as usize).max((f / 1.06 / bin) as usize + 1),
            );
            let s = &spec[lo.min(spec.len() - 1)..hi.min(spec.len())];
            let p = s.iter().map(|c| c.norm_sqr()).sum::<f32>() / s.len().max(1) as f32;
            10.0 * p.max(1e-12).log10()
        };
        let pts: Vec<(f32, f32)> = (0..=120)
            .map(|i| 20.0 * 1000f32.powf(i as f32 / 120.0))
            .map(|f| (f, mag(f)))
            .collect();
        let top = pts.iter().fold(f32::MIN, |a, p| a.max(p.1));
        Picture::Response(pts.into_iter().map(|(f, d)| (f, d - top + 3.0)).collect())
    };
    cache.lock().ok()?.insert(asset, pic.clone());
    Some(pic)
}

// -- the NAM amp ---------------------------------------------------------------------------------------------------

/// A capture at another rate than the device's: resampled to it, run, and back.
struct ResampledModel {
    model: Model,
    up: Fft<f32>,
    down: Fft<f32>,
    fifo_in: VecDeque<f32>,
    mid: VecDeque<f32>,
    fifo_out: VecDeque<f32>,
    a: Vec<f32>,
    b: Vec<f32>,
    c: Vec<f32>,
}

const FIFO_CAP: usize = 16384;

impl ResampledModel {
    fn new(model: Model, device: u32, capture: u32) -> Option<ResampledModel> {
        let up = Fft::<f32>::new(device as usize, capture as usize, 128, 1, FixedSync::Both).ok()?;
        let down = Fft::<f32>::new(capture as usize, device as usize, 128, 1, FixedSync::Both).ok()?;
        let n_up_in = up.input_frames_next();
        let n_up_out = up.output_frames_next();
        let n_down_in = down.input_frames_next();
        let n_down_out = down.output_frames_next();
        let mut r = ResampledModel {
            model,
            fifo_in: VecDeque::with_capacity(FIFO_CAP),
            mid: VecDeque::with_capacity(FIFO_CAP),
            fifo_out: VecDeque::with_capacity(FIFO_CAP),
            a: vec![0.0; n_up_in.max(n_down_in)],
            b: vec![0.0; n_up_out.max(n_down_in)],
            c: vec![0.0; n_down_out],
            up,
            down,
        };
        // primed so the output never runs dry: a whole input chunk and a whole output chunk
        r.fifo_out.extend(std::iter::repeat_n(0.0, n_up_in + n_down_out));
        Some(r)
    }

    /// Samples of delay this adds at the device rate.
    fn latency(&self) -> usize {
        self.fifo_out.len()
    }

    fn process(&mut self, x: &mut [f32]) {
        for &v in x.iter() {
            if self.fifo_in.len() < FIFO_CAP {
                self.fifo_in.push_back(v);
            }
        }
        let n_up_in = self.up.input_frames_next();
        let n_up_out = self.up.output_frames_next();
        while self.fifo_in.len() >= n_up_in {
            for (o, v) in self.a[..n_up_in].iter_mut().zip(self.fifo_in.drain(..n_up_in)) {
                *o = v;
            }
            let ok = match (
                InterleavedSlice::new(&self.a[..n_up_in], 1, n_up_in),
                InterleavedSlice::new_mut(&mut self.b[..n_up_out], 1, n_up_out),
            ) {
                (Ok(i), Ok(mut o)) => self.up.process_into_buffer(&i, &mut o, None).is_ok(),
                _ => false,
            };
            if !ok {
                self.b[..n_up_out].fill(0.0);
            }
            for &v in &self.b[..n_up_out] {
                if self.mid.len() < FIFO_CAP {
                    self.mid.push_back(v);
                }
            }
            let n_down_in = self.down.input_frames_next();
            let n_down_out = self.down.output_frames_next();
            while self.mid.len() >= n_down_in {
                for (o, v) in self.a[..n_down_in].iter_mut().zip(self.mid.drain(..n_down_in)) {
                    *o = v;
                }
                self.model.process_buffer(&mut self.a[..n_down_in]);
                let ok = match (
                    InterleavedSlice::new(&self.a[..n_down_in], 1, n_down_in),
                    InterleavedSlice::new_mut(&mut self.c[..n_down_out], 1, n_down_out),
                ) {
                    (Ok(i), Ok(mut o)) => self.down.process_into_buffer(&i, &mut o, None).is_ok(),
                    _ => false,
                };
                if !ok {
                    self.c.fill(0.0);
                }
                for &v in &self.c[..n_down_out] {
                    if self.fifo_out.len() < FIFO_CAP {
                        self.fifo_out.push_back(v);
                    }
                }
            }
        }
        for v in x.iter_mut() {
            *v = self.fifo_out.pop_front().unwrap_or(0.0);
        }
    }

    fn reset(&mut self) {
        self.model.reset();
        self.up.reset();
        self.down.reset();
        let lat = self.fifo_out.len();
        self.fifo_in.clear();
        self.mid.clear();
        self.fifo_out.clear();
        self.fifo_out.extend(std::iter::repeat_n(0.0, lat));
    }
}

enum Runner {
    Direct(Model),
    Resampled(Box<ResampledModel>),
    /// No capture: the signal passes.
    Through,
}

/// The dry copy a NAM pedal's blend needs, at most this many samples at a time (longer buffers are split).
const DRY_MAX: usize = 512;

struct NamAmp {
    run: Runner,
    /// An amp (Bass / Mid / Treble, Gate) or a pedal (Blend, Clean low).
    pedal: bool,
    /// The calibration gains (input, output) and the knobs' (input, output), smoothed.
    cal_in: f32,
    cal_out: f32,
    gin: dsp::Smooth,
    gout: dsp::Smooth,
    dc: dsp::OnePole,
    rate: f32,
    /// The post-capture EQ (low shelf 100 Hz, peak 650 Hz, high shelf 3 kHz); skipped while all are at 0 dB.
    eq: [Biquad; 3],
    eq_db: [f32; 3],
    eq_on: bool,
    /// The gate in front (an amp's); `gate_on` while its threshold is above the knob's bottom (Off).
    gate: Box<dyn Pedal>,
    gate_on: bool,
    /// The pedal's blend with the dry input, delayed by the resampler's latency to line up.
    blend: dsp::Blend,
    dry: Vec<f32>,
    dry_line: dsp::Line,
    lat: usize,
}

/// The NAM amp's Gate knob at its bottom is Off.
const GATE_OFF: f32 = -89.5;

impl NamAmp {
    fn run_model(&mut self, x: &mut [f32]) {
        for v in x.iter_mut() {
            *v *= self.cal_in * self.gin.tick();
        }
        match &mut self.run {
            Runner::Direct(m) => m.process_buffer(x),
            Runner::Resampled(r) => r.process(x),
            Runner::Through => {}
        }
        for v in x.iter_mut() {
            *v = dsp::flush(self.dc.hp(*v * self.cal_out * self.gout.tick()));
        }
        if self.eq_on {
            for v in x.iter_mut() {
                let mut s = *v;
                for b in &mut self.eq {
                    s = b.tick(s);
                }
                *v = s;
            }
        }
    }
}

impl Pedal for NamAmp {
    fn process(&mut self, x: &mut [f32]) {
        if matches!(self.run, Runner::Through) {
            return;
        }
        if self.gate_on {
            self.gate.process(x);
        }
        if !self.blend.active() {
            self.run_model(x);
            return;
        }
        for c in x.chunks_mut(DRY_MAX) {
            let n = c.len();
            for (d, &v) in self.dry[..n].iter_mut().zip(c.iter()) {
                self.dry_line.push(v);
                *d = self.dry_line.at(self.lat);
            }
            self.run_model(c);
            for (v, &d) in c.iter_mut().zip(self.dry[..n].iter()) {
                *v = self.blend.tick(*v, d);
            }
        }
    }

    fn set(&mut self, v: &[f32; MAX_KNOBS]) {
        self.gin.target = db(v[0]);
        self.gout.target = db(v[1]);
        if self.pedal {
            // input, output, blend, clean low, gain
            self.blend.set((v[2] / 100.0).clamp(0.0, 1.0), v[3]);
        } else {
            // input, output, bass, mid, treble, gate, gain
            let want = [v[2], v[3], v[4]];
            if want != self.eq_db {
                self.eq_db = want;
                let r = self.rate;
                self.eq[0].set(
                    r,
                    Filter::LowShelf {
                        f: 100.0,
                        db: want[0],
                    },
                );
                self.eq[1].set(
                    r,
                    Filter::Peak {
                        f: 650.0,
                        q: 0.7,
                        db: want[1],
                    },
                );
                self.eq[2].set(
                    r,
                    Filter::HighShelf {
                        f: 3000.0,
                        db: want[2],
                    },
                );
            }
            self.eq_on = want.iter().any(|d| d.abs() > 0.01);
            // (a value outside the knob's range, as from a zeroed knob array, is Off too)
            self.gate_on = (GATE_OFF..=-29.0).contains(&v[5]);
            let mut g = [0.0; MAX_KNOBS];
            g[0] = v[5];
            g[1] = 80.0;
            // (all the way down, listening to its own input: the capture's gate as it always was, P41.1)
            g[2] = -90.0;
            g[3] = 1.0;
            self.gate.set(&g);
        }
    }

    fn reset(&mut self) {
        match &mut self.run {
            Runner::Direct(m) => m.reset(),
            Runner::Resampled(r) => r.reset(),
            Runner::Through => {}
        }
        self.gin.snap();
        self.gout.snap();
        self.dc.reset();
        for b in &mut self.eq {
            b.reset();
        }
        self.gate.reset();
        self.blend.reset();
        self.dry_line.clear();
        self.warm();
    }
}

impl NamAmp {
    /// Run the receptive field's worth of silence, so the first notes aren't the start-up transient.
    fn warm(&mut self) {
        let n = match &self.run {
            Runner::Direct(m) => m.receptive_field(),
            Runner::Resampled(r) => r.model.receptive_field() + r.latency(),
            Runner::Through => 0,
        };
        let mut z = [0.0f32; 256];
        let mut left = n;
        while left > 0 {
            let k = left.min(z.len());
            z[..k].fill(0.0);
            match &mut self.run {
                Runner::Direct(m) => m.process_buffer(&mut z[..k]),
                Runner::Resampled(r) => r.process(&mut z[..k]),
                Runner::Through => {}
            }
            left -= k;
        }
    }
}

/// A NAM amp for capture `asset` at the device rate (allocates: off the audio thread). Passes the signal through
/// when the capture isn't loaded.
pub fn make_amp(asset: u32, rate: f32) -> Box<dyn Pedal> {
    make_capture(asset, rate, true)
}

/// A NAM pedal: as an amp, its output brought to `PEDAL_TARGET`.
pub fn make_pedal_capture(asset: u32, rate: f32) -> Box<dyn Pedal> {
    make_capture(asset, rate, false)
}

pub fn pedal_nam(rate: f32) -> Box<dyn Pedal> {
    make_pedal_capture(0, rate)
}

fn make_capture(asset: u32, rate: f32, normalise: bool) -> Box<dyn Pedal> {
    let loaded = match registry().read().unwrap_or_else(|e| e.into_inner()).get(&asset) {
        Some(Loaded::Nam { model, info }) => Some((model.clone(), info.clone())),
        _ => None,
    };
    let mut amp = NamAmp {
        run: Runner::Through,
        pedal: !normalise,
        cal_in: 1.0,
        cal_out: 1.0,
        gin: dsp::Smooth::new(rate, 30.0, 1.0),
        gout: dsp::Smooth::new(rate, 30.0, 1.0),
        dc: dsp::OnePole::new(rate, 5.0),
        rate,
        eq: [Biquad::default(); 3],
        eq_db: [f32::NAN; 3],
        eq_on: false,
        gate: super::dynamics::gate(rate),
        gate_on: false,
        blend: dsp::Blend::new(rate),
        dry: vec![0.0; DRY_MAX],
        dry_line: dsp::Line::new(4),
        lat: 0,
    };
    if let Some((m, info)) = loaded
        && let Ok(model) = Model::from_nam(&m)
    {
        let cap = info.rate.round() as u32;
        let dev = rate.round() as u32;
        amp.run = if cap == dev || cap == 0 {
            Runner::Direct(model)
        } else {
            match ResampledModel::new(model, dev, cap) {
                Some(r) => Runner::Resampled(Box::new(r)),
                None => Runner::Through,
            }
        };
        amp.cal_in = input_gain(input_dbu(), info.input_dbu);
        let target = if normalise { LOUDNESS_TARGET } else { PEDAL_TARGET };
        amp.cal_out = info.loudness.map(|l| db(target - l)).unwrap_or(1.0) * trim(asset);
        if let Runner::Resampled(r) = &amp.run {
            amp.lat = r.latency();
        }
        amp.dry_line = dsp::Line::new(amp.lat + 4);
        amp.warm();
    }
    Box::new(amp)
}

/// The catalogue's maker (no capture: through; `make_pedal` uses `make_amp` with the block's asset).
pub fn amp_nam(rate: f32) -> Box<dyn Pedal> {
    make_amp(0, rate)
}

// -- the IR cab ----------------------------------------------------------------------------------------------------

/// Convolution with no latency: taps `[0, IR_BLOCK)` directly, the rest in FFT partitions of `IR_BLOCK` (each
/// partition's share of block k is ready at the start of block k + 1, exactly when its delay brings it due).
struct Convolver {
    head: Vec<f32>,
    /// The last `IR_BLOCK` inputs, newest at `hpos - 1` (a ring for the direct taps).
    hist: Vec<f32>,
    hpos: usize,
    parts: Vec<Vec<Complex32>>,
    /// The spectra of the last blocks (a ring, as many as partitions).
    fdl: Vec<Vec<Complex32>>,
    fpos: usize,
    /// The block being filled, and the one before it (the FFT's window).
    cur: Vec<f32>,
    prev: Vec<f32>,
    n: usize,
    /// The tail's output for the current block.
    tail: Vec<f32>,
    fwd: Arc<dyn RealToComplex<f32>>,
    inv: Arc<dyn ComplexToReal<f32>>,
    win: Vec<f32>,
    spec: Vec<Complex32>,
    acc: Vec<Complex32>,
    scratch_f: Vec<Complex32>,
    scratch_i: Vec<Complex32>,
}

impl Convolver {
    fn new(ir: &[f32]) -> Convolver {
        let b = IR_BLOCK;
        let mut planner = RealFftPlanner::<f32>::new();
        let fwd = planner.plan_fft_forward(2 * b);
        let inv = planner.plan_fft_inverse(2 * b);
        let head: Vec<f32> = ir
            .iter()
            .take(b)
            .copied()
            .chain(std::iter::repeat(0.0))
            .take(b)
            .collect();
        let mut parts = Vec::new();
        let mut win = fwd.make_input_vec();
        let mut scratch_f = fwd.make_scratch_vec();
        for chunk in ir.get(b..).unwrap_or(&[]).chunks(b) {
            win.fill(0.0);
            win[..chunk.len()].copy_from_slice(chunk);
            let mut s = fwd.make_output_vec();
            let _ = fwd.process_with_scratch(&mut win, &mut s, &mut scratch_f);
            parts.push(s);
        }
        let np = parts.len().max(1);
        Convolver {
            head,
            hist: vec![0.0; b],
            hpos: 0,
            fdl: (0..np).map(|_| fwd.make_output_vec()).collect(),
            fpos: 0,
            cur: vec![0.0; b],
            prev: vec![0.0; b],
            n: 0,
            tail: vec![0.0; b],
            spec: fwd.make_output_vec(),
            acc: fwd.make_output_vec(),
            scratch_i: inv.make_scratch_vec(),
            scratch_f,
            win,
            parts,
            fwd,
            inv,
        }
    }

    #[inline]
    fn tick(&mut self, x: f32) -> f32 {
        let b = IR_BLOCK;
        self.hist[self.hpos] = x;
        self.hpos = (self.hpos + 1) % b;
        // the direct taps: h[j] x[t - j], newest first
        let mut y = 0.0;
        let mut i = self.hpos;
        for &h in &self.head {
            i = if i == 0 { b - 1 } else { i - 1 };
            y += h * self.hist[i];
        }
        y += self.tail[self.n];
        self.cur[self.n] = x;
        self.n += 1;
        if self.n == b {
            self.block();
            self.n = 0;
        }
        y
    }

    /// A block is in: its spectrum into the delay line, and the tail's output for the next block.
    fn block(&mut self) {
        let b = IR_BLOCK;
        if self.parts.is_empty() {
            self.tail.fill(0.0);
            std::mem::swap(&mut self.prev, &mut self.cur);
            return;
        }
        self.win[..b].copy_from_slice(&self.prev);
        self.win[b..].copy_from_slice(&self.cur);
        let _ = self
            .fwd
            .process_with_scratch(&mut self.win, &mut self.spec, &mut self.scratch_f);
        let np = self.parts.len();
        self.fpos = (self.fpos + 1) % np;
        self.fdl[self.fpos].copy_from_slice(&self.spec);
        self.acc.iter_mut().for_each(|c| *c = Complex32::new(0.0, 0.0));
        for (p, h) in self.parts.iter().enumerate() {
            let x = &self.fdl[(self.fpos + np - p) % np];
            for ((a, &xv), &hv) in self.acc.iter_mut().zip(x.iter()).zip(h.iter()) {
                *a += xv * hv;
            }
        }
        // (realfft's inverse wants the DC and Nyquist bins real)
        if let Some(c) = self.acc.first_mut() {
            c.im = 0.0;
        }
        if let Some(c) = self.acc.last_mut() {
            c.im = 0.0;
        }
        let _ = self
            .inv
            .process_with_scratch(&mut self.acc, &mut self.win, &mut self.scratch_i);
        let k = 1.0 / (2 * b) as f32;
        for (t, &w) in self.tail.iter_mut().zip(&self.win[b..]) {
            *t = w * k;
        }
        std::mem::swap(&mut self.prev, &mut self.cur);
    }

    fn reset(&mut self) {
        self.hist.fill(0.0);
        self.cur.fill(0.0);
        self.prev.fill(0.0);
        self.tail.fill(0.0);
        self.n = 0;
        for f in &mut self.fdl {
            f.iter_mut().for_each(|c| *c = Complex32::new(0.0, 0.0));
        }
    }
}

struct IrCab {
    conv: Option<Convolver>,
    trim: f32,
    gain: dsp::Smooth,
    /// (P37.2: two-pole cuts, 12 dB an octave, as the NAM plugin's; they were one-pole)
    lo: Biquad,
    hi: Biquad,
    cuts: (f32, f32),
    /// The DI blended with the IR (a bass's usual DI + mic), through a gentle 9 kHz low-pass.
    di: f32,
    di_lp: Biquad,
    rate: f32,
}

impl Pedal for IrCab {
    fn process(&mut self, x: &mut [f32]) {
        let Some(c) = &mut self.conv else {
            return;
        };
        for v in x.iter_mut() {
            let x0 = *v;
            let y = c.tick(x0);
            let y = self.hi.tick(self.lo.tick(y));
            let mut y = y * self.trim;
            if self.di > 0.0 {
                let k = 1.0 + 0.33 * 4.0 * self.di * (1.0 - self.di);
                y = (y * (1.0 - self.di) + self.di_lp.tick(x0) * self.di) * k;
            }
            *v = dsp::flush(y * self.gain.tick());
        }
    }

    fn set(&mut self, v: &[f32; MAX_KNOBS]) {
        self.gain.target = db(v[0]);
        if self.cuts != (v[1], v[2]) {
            self.cuts = (v[1], v[2]);
            self.lo.set(
                self.rate,
                Filter::HighPass {
                    f: v[1].clamp(10.0, 1000.0),
                    q: std::f32::consts::FRAC_1_SQRT_2,
                },
            );
            self.hi.set(
                self.rate,
                Filter::LowPass {
                    f: v[2].clamp(1000.0, self.rate * 0.45),
                    q: std::f32::consts::FRAC_1_SQRT_2,
                },
            );
        }
        self.di = (v[3] / 100.0).clamp(0.0, 1.0);
    }

    fn reset(&mut self) {
        if let Some(c) = &mut self.conv {
            c.reset();
        }
        self.gain.snap();
        self.lo.reset();
        self.hi.reset();
        self.di_lp.reset();
    }
}

/// An IR's samples at the device rate: resampled, cut to `IR_MAX_S`, a short fade at the cut, unit energy (so IRs
/// of any loudness come out alike; the cab's level knob is on top).
pub fn prepare_ir(samples: &[f32], from: u32, to: u32) -> Vec<f32> {
    let mut ir = if from == to || from == 0 {
        samples.to_vec()
    } else {
        crate::decode::resample(samples.to_vec(), 1, from, to).unwrap_or_else(|_| samples.to_vec())
    };
    let max = (IR_MAX_S * to as f32) as usize;
    if ir.len() > max {
        ir.truncate(max);
        let fade = (to as usize / 100).min(ir.len());
        let n = ir.len();
        for (i, v) in ir[n - fade..].iter_mut().enumerate() {
            *v *= 1.0 - i as f32 / fade as f32;
        }
    }
    let e: f32 = ir.iter().map(|v| v * v).sum::<f32>().sqrt();
    if e > 0.0 {
        for v in &mut ir {
            *v /= e;
        }
    }
    ir
}

/// An IR cab for IR `asset` at the device rate (allocates: off the audio thread); through when it isn't loaded.
pub fn make_cab(asset: u32, rate: f32) -> Box<dyn Pedal> {
    let loaded = match registry().read().unwrap_or_else(|e| e.into_inner()).get(&asset) {
        Some(Loaded::Ir { samples, rate }) => Some((samples.clone(), *rate)),
        _ => None,
    };
    let conv = loaded.map(|(s, from)| Convolver::new(&prepare_ir(&s, from, rate.round() as u32)));
    Box::new(IrCab {
        conv,
        trim: trim(asset),
        gain: dsp::Smooth::new(rate, 30.0, 1.0),
        lo: Biquad::default(),
        hi: Biquad::default(),
        cuts: (-1.0, -1.0),
        di: 0.0,
        di_lp: Biquad::new(
            rate,
            Filter::LowPass {
                f: (9000.0f32).min(rate * 0.45),
                q: 0.7,
            },
        ),
        rate,
    })
}

pub fn cab_ir(rate: f32) -> Box<dyn Pedal> {
    make_cab(0, rate)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    #[test]
    fn the_reference_level_sets_the_capture_input() {
        // today's behaviour: +12 dBu into a capture trained at +12 is unity, into one at +18 is 6 dB down
        assert!((input_gain(INPUT_DBU, Some(12.0)) - 1.0).abs() < 1e-6);
        assert!((input_gain(INPUT_DBU, Some(18.0)) - db(-6.0)).abs() < 1e-6);
        // a +4 dBu interface: 8 dB less into the same capture; -10 dBV (-7.78 dBu) about 19.8 dB less
        assert!((input_gain(4.0, Some(12.0)) - db(-8.0)).abs() < 1e-6);
        let g = 20.0 * input_gain(-7.78, Some(12.0)).log10();
        assert!((g + 19.78).abs() < 1e-3);
        // a capture that doesn't say: as is, whatever the reference
        assert_eq!(input_gain(4.0, None), 1.0);
        // the setting as kept: clamped, the default for nonsense (setting it is global: not done in tests, a board
        // made meanwhile in another test would take it)
        assert_eq!(clamp_dbu(4.0), 4.0);
        assert_eq!(clamp_dbu(99.0), INPUT_DBU_RANGE.1);
        assert_eq!(clamp_dbu(-99.0), INPUT_DBU_RANGE.0);
        assert_eq!(clamp_dbu(f32::NAN), INPUT_DBU);
        assert_eq!(input_dbu(), INPUT_DBU);
    }

    /// A tiny WaveNet capture (from nam-rs's example): one layer, one channel, a gain of 2 through a ReLU-free path.
    pub(crate) fn tiny_capture(extra: &str) -> String {
        format!(
            r#"{{
            "version": "0.5.4", "architecture": "WaveNet",
            "config": {{
                "layers": [{{
                    "input_size": 1, "condition_size": 1, "channels": 1, "head_size": 1,
                    "kernel_size": 1, "dilations": [1], "activation": "ReLU",
                    "gated": false, "head_bias": false
                }}],
                "head": null, "head_scale": 1.0
            }},
            "weights": [1.0, 2.0, 0.0, 0.0, 1.0, 0.0, 1.0, 1.0]{extra}
        }}"#
        )
    }

    fn run(p: &mut Box<dyn Pedal>, input: &[f32]) -> Vec<f32> {
        let mut out = Vec::new();
        for c in input.chunks(97) {
            let mut b = c.to_vec();
            p.process(&mut b);
            out.extend(b);
        }
        out
    }

    #[test]
    fn a_capture_registers_and_runs() {
        let info = register_nam(9001, &tiny_capture(r#", "sample_rate": 48000"#)).unwrap();
        assert_eq!(info.rate, 48000.0);
        assert_eq!(info.architecture, "WaveNet");
        let mut p = make_amp(9001, 48000.0);
        let mut v = [0.0; MAX_KNOBS];
        p.set(&v);
        p.reset();
        let x: Vec<f32> = (0..4800).map(|i| 0.3 * (i as f32 * 0.05).sin()).collect();
        let y = run(&mut p, &x);
        assert!(y.iter().all(|v| v.is_finite()));
        let e: f32 = y[2000..].iter().map(|v| v * v).sum();
        assert!(e > 1.0, "the capture made sound ({e})");
        // the output knob turns it down
        v[1] = -20.0;
        p.set(&v);
        let y2 = run(&mut p, &x);
        let e2: f32 = y2[3000..].iter().map(|v| v * v).sum();
        let e1: f32 = y[3000..].iter().map(|v| v * v).sum();
        assert!(e2 < e1 * 0.05, "{e2} vs {e1}");
        unregister(9001);
    }

    #[test]
    fn a_capture_at_another_rate_is_resampled() {
        register_nam(9002, &tiny_capture(r#", "sample_rate": 48000"#)).unwrap();
        let mut p = make_amp(9002, 44100.0);
        p.set(&[0.0; MAX_KNOBS]);
        p.reset();
        let x: Vec<f32> = (0..44100)
            .map(|i| 0.3 * (i as f32 * 2.0 * std::f32::consts::PI * 220.0 / 44100.0).sin())
            .collect();
        let y = run(&mut p, &x);
        assert_eq!(y.len(), x.len());
        let e: f32 = y[10000..].iter().map(|v| v * v).sum();
        assert!(e > 10.0, "sound came through the resampler ({e})");
        unregister(9002);
    }

    #[test]
    fn a_missing_capture_passes_the_signal() {
        let mut p = make_amp(424242, 48000.0);
        let x = vec![0.25f32; 100];
        assert_eq!(run(&mut p, &x), x);
    }

    #[test]
    fn junk_is_not_a_capture() {
        assert!(register_nam(9003, "{}").is_err());
        assert!(inspect("not json").is_err());
        assert!(!is_registered(9003));
    }

    /// The convolver against a plain convolution, for an IR longer than several partitions.
    #[test]
    fn the_convolver_matches_a_direct_convolution() {
        let ir: Vec<f32> = (0..700)
            .map(|i| ((i * 37 % 11) as f32 - 5.0) / 10.0 * (-(i as f32) / 200.0).exp())
            .collect();
        let x: Vec<f32> = (0..3000).map(|i| ((i * 13 % 7) as f32 - 3.0) / 3.0).collect();
        let mut c = Convolver::new(&ir);
        let got: Vec<f32> = x.iter().map(|&v| c.tick(v)).collect();
        for t in 0..x.len() {
            let want: f32 = (0..ir.len().min(t + 1)).map(|j| ir[j] * x[t - j]).sum();
            assert!((got[t] - want).abs() < 1e-3, "t={t}: {} vs {want}", got[t]);
        }
    }

    #[test]
    fn an_ir_cab_runs_from_a_wav() {
        // a WAV of a decaying click, 44.1 kHz, made here
        let ir: Vec<i16> = (0..2000)
            .map(|i| ((20000.0 * (-(i as f32) / 300.0).exp()) as i16) * if i % 2 == 0 { 1 } else { -1 })
            .collect();
        let dir = std::env::temp_dir().join(format!("qrs-ir-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("ir.wav");
        crate::decode::write_wav_i16(&path, 44100, 1, &ir).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        let secs = register_ir(9004, &bytes).unwrap();
        assert!((secs - 2000.0 / 44100.0).abs() < 1e-3);
        let mut p = make_cab(9004, 48000.0);
        let mut v = [0.0; MAX_KNOBS];
        v[1] = 20.0;
        v[2] = 16000.0;
        p.set(&v);
        p.reset();
        let x: Vec<f32> = (0..4800).map(|i| if i % 480 == 0 { 0.5 } else { 0.0 }).collect();
        let y = run(&mut p, &x);
        assert!(y.iter().all(|v| v.is_finite()));
        assert!(y.iter().map(|v| v.abs()).fold(0.0, f32::max) > 0.01);
        unregister(9004);
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[cfg(test)]
mod board_tests {
    use super::tests::tiny_capture;
    use super::*;
    use crate::board::{Block, Board, BoardCmd, BoardProcessor, diff};
    use crate::rt::ToneProcessor;

    #[test]
    fn a_board_runs_its_capture_and_swaps_it() {
        register_nam(9101, &tiny_capture("")).unwrap();
        register_nam(9102, &tiny_capture("")).unwrap();
        let mut amp = Block::of("amp_nam").unwrap();
        amp.asset = 9101;
        let b = Board::new(vec![amp]);
        // the asset is kept through JSON
        assert_eq!(Board::from_json(&b.to_json()).blocks[0].asset, 9101);
        let mut p = BoardProcessor::new(&b, 48000);
        let mut x: Vec<f32> = (0..4800).map(|i| 0.2 * (i as f32 * 0.05).sin()).collect();
        let x0 = x.clone();
        p.process(&mut x);
        assert!(
            x.iter().zip(&x0).any(|(a, b)| (a - b).abs() > 1e-3),
            "the capture changed the sound"
        );
        // another capture on the same block: a replace, built off the audio thread
        let mut b2 = b.clone();
        b2.blocks[0].asset = 9102;
        let cmds = diff(&b, &b2, 48000).unwrap();
        assert!(matches!(cmds.as_slice(), [BoardCmd::Replace { .. }, ..]));
        unregister(9101);
        unregister(9102);
    }
}
