//! `quickrs-cli board listen` (P37.2, "listen to the combinations"): synthetic DI material (Karplus-Strong guitar
//! and bass: chords, palm-muted chugs, a lick, finger and pick bass, slap, a 5-string's low B) played through every
//! starting board and a matrix of chains (each amp clean and driven through two cabs, every drive and fuzz into an
//! amp, the bass chains, and NAM captures when the owner's library has them), written as WAVs with a measurement of
//! each: integrated loudness (BS.1770-4), peak and clipping, DC, where the energy sits, how much of a bass's
//! fundamental survives, how fast it runs. Then the stress tests: aliasing (high sines through high gain: the
//! energy away from the harmonics), the noise floor (an idle input's hiss), silence after a note (denormals: the
//! time per block must not climb). Writes `INDEX.md` (a table to listen from) and `metrics.csv` beside the WAVs.
//!
//! Captures are read from the library in place (`%APPDATA%\quickRS\data\captures`, or `--captures`), never copied.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::time::Instant;

use qrs_analysis::testsig;

use crate::board_cmd::dry;
use qrs_audio::board::{self, Block, Board, Kind, catalog, nam};

const RATE: u32 = 48000;
const R: f32 = RATE as f32;

// -- material -------------------------------------------------------------------------------------------------------

fn peak_to(x: &mut [f32], peak: f32) {
    let p = x.iter().fold(0.0f32, |m, v| m.max(v.abs())).max(1e-9);
    for v in x.iter_mut() {
        *v *= peak / p;
    }
}

/// A bass line of plucks (MIDI notes, eighths at 120 bpm), finger (soft) or pick.
fn bass_line(notes: &[f64], soft: f32, seed: u32) -> Vec<f32> {
    let step = (0.25 * R) as usize;
    let mut x = vec![0.0f32; step * notes.len() + RATE as usize];
    for (i, &m) in notes.iter().enumerate() {
        let n = testsig::ks_pluck(m, RATE, 0.6, 0.3, soft, seed + i as u32);
        testsig::mix_at(&mut x, i * step, &n);
    }
    peak_to(&mut x, 0.316);
    x
}

/// Slap and pop: a thumb's sharp hit on the low strings (a click on the pluck) and popped octaves.
fn slap() -> Vec<f32> {
    let step = (0.25 * R) as usize;
    let seq: [(f64, bool); 8] = [
        (28.0, false),
        (40.0, true),
        (28.0, false),
        (28.0, false),
        (31.0, false),
        (43.0, true),
        (33.0, false),
        (45.0, true),
    ];
    let mut x = vec![0.0f32; step * seq.len() + RATE as usize];
    let mut noise = testsig::Noise::new(11);
    for (i, &(m, pop)) in seq.iter().enumerate() {
        let mut n = testsig::ks_pluck(m, RATE, 0.4, 0.3, 0.0, 30 + i as u32);
        // the thumb's (or the pop's) click: a 2 ms burst of bright noise
        let click = (0.002 * R) as usize;
        for (k, v) in n.iter_mut().take(click).enumerate() {
            *v += noise.sample() * (if pop { 0.5 } else { 0.35 }) * (1.0 - k as f32 / click as f32);
        }
        testsig::mix_at(&mut x, i * step, &n);
    }
    peak_to(&mut x, 0.316);
    x
}

struct Material {
    name: &'static str,
    x: Vec<f32>,
    bass: bool,
}

fn material() -> Vec<Material> {
    use crate::board_cmd::{chords, chug, lick};
    vec![
        Material {
            name: "guitar_chords",
            x: dry(&chords(), false),
            bass: false,
        },
        Material {
            name: "guitar_chug",
            x: dry(&chug(), false),
            bass: false,
        },
        Material {
            name: "guitar_lick",
            x: dry(&lick(), false),
            bass: false,
        },
        Material {
            name: "bass_finger",
            x: bass_line(
                &[
                    28.0, 28.0, 35.0, 33.0, 31.0, 28.0, 38.0, 40.0, 28.0, 28.0, 31.0, 33.0,
                ],
                0.6,
                5,
            ),
            bass: true,
        },
        Material {
            name: "bass_pick",
            x: bass_line(
                &[
                    28.0, 28.0, 28.0, 31.0, 33.0, 33.0, 26.0, 26.0, 28.0, 28.0, 28.0, 35.0,
                ],
                0.0,
                9,
            ),
            bass: true,
        },
        Material {
            name: "bass_slap",
            x: slap(),
            bass: true,
        },
        Material {
            // a 5-string: low B, C, D, E
            name: "bass_low_b",
            x: bass_line(&[23.0, 23.0, 24.0, 26.0, 23.0, 23.0, 28.0, 23.0], 0.5, 21),
            bass: true,
        },
    ]
}

// -- the amps' loudness table (P37.6c) --------------------------------------------------------------------------------

/// `board loudness`: every amp of ours alone into its home cab (`board::loudness::HOME_CAB`), its gain (or volume)
/// at 0, 1, .. 10 and the rest at defaults, on the guitar chords or the finger bass line: integrated LUFS. Printed as
/// the table `crates/qrs-audio/src/board/loudness.rs` holds (the room's level match on an amp swap reads it).
pub fn loudness_table() -> String {
    let guitar = dry(&crate::board_cmd::chords(), false);
    let bass = bass_line(
        &[
            28.0, 28.0, 35.0, 33.0, 31.0, 28.0, 38.0, 40.0, 28.0, 28.0, 31.0, 33.0,
        ],
        0.6,
        5,
    );
    let mut s = String::from("pub const AMP_LUFS: &[(&str, [f32; 11])] = &[\n");
    for (_, m) in catalog::of_kind(Kind::Amp) {
        let x = if board::loudness::is_bass_amp(m.id) {
            &bass
        } else {
            &guitar
        };
        let key = board::loudness::gain_key(m);
        let mut row = Vec::new();
        for g in 0..=10 {
            let board = Board::new(vec![
                b(m.id).with(key, g as f32),
                b(board::loudness::home_cab(m.id)),
            ]);
            let (y, _, _) = render_timed(&board, x);
            row.push(format!("{:.1}", lufs(&y)));
        }
        let _ = writeln!(s, "    (\"{}\", [{}]),", m.id, row.join(", "));
    }
    s.push_str("];\n");
    s
}

// -- measuring ------------------------------------------------------------------------------------------------------

/// Integrated loudness (ITU-R BS.1770-4: K-weighting, 400 ms blocks every 100 ms, the -70 LUFS and relative -10
/// LU gates) at 48 kHz.
pub fn lufs(x: &[f32]) -> f64 {
    // the two K-weighting stages' coefficients at 48 kHz, as the standard gives them
    let shelf = (
        [1.53512485958697, -2.69169618940638, 1.19839281085285],
        [-1.69065929318241, 0.73248077421585],
    );
    let hp = ([1.0, -2.0, 1.0], [-1.99004745483398, 0.99007225036621]);
    let biquad = |x: &[f64], (b, a): ([f64; 3], [f64; 2])| {
        let (mut x1, mut x2, mut y1, mut y2) = (0.0, 0.0, 0.0, 0.0);
        x.iter()
            .map(|&v| {
                let y = b[0] * v + b[1] * x1 + b[2] * x2 - a[0] * y1 - a[1] * y2;
                x2 = x1;
                x1 = v;
                y2 = y1;
                y1 = y;
                y
            })
            .collect::<Vec<f64>>()
    };
    let k = biquad(
        &biquad(&x.iter().map(|&v| v as f64).collect::<Vec<_>>(), shelf),
        hp,
    );
    let (blk, hop) = ((0.4 * R) as usize, (0.1 * R) as usize);
    let mut z = Vec::new();
    let mut i = 0;
    while i + blk <= k.len() {
        z.push(k[i..i + blk].iter().map(|v| v * v).sum::<f64>() / blk as f64);
        i += hop;
    }
    let l = |z: f64| -0.691 + 10.0 * z.max(1e-20).log10();
    let abs: Vec<f64> = z.iter().copied().filter(|&v| l(v) > -70.0).collect();
    if abs.is_empty() {
        return -100.0;
    }
    let mean = abs.iter().sum::<f64>() / abs.len() as f64;
    let rel: Vec<f64> = abs.iter().copied().filter(|&v| l(v) > l(mean) - 10.0).collect();
    l(rel.iter().sum::<f64>() / rel.len().max(1) as f64)
}

/// A power spectrum averaged over 8192-point Hann frames (bin = R / 8192).
pub(crate) fn spectrum(x: &[f32]) -> Vec<f64> {
    use realfft::RealFftPlanner;
    let n = 8192;
    let mut planner = RealFftPlanner::<f64>::new();
    let fft = planner.plan_fft_forward(n);
    let mut acc = vec![0.0f64; n / 2 + 1];
    let mut i = 0;
    while i + n <= x.len() {
        let mut buf: Vec<f64> = (0..n)
            .map(|k| x[i + k] as f64 * (0.5 - 0.5 * (std::f64::consts::TAU * k as f64 / n as f64).cos()))
            .collect();
        let mut spec = fft.make_output_vec();
        if fft.process(&mut buf, &mut spec).is_ok() {
            for (a, c) in acc.iter_mut().zip(&spec) {
                *a += c.norm_sqr();
            }
        }
        i += n / 2;
    }
    acc
}

pub(crate) fn band(spec: &[f64], lo: f64, hi: f64) -> f64 {
    let bin = R as f64 / 8192.0;
    let total: f64 = spec.iter().sum::<f64>().max(1e-30);
    spec.iter()
        .enumerate()
        .filter(|(k, _)| *k as f64 * bin >= lo && (*k as f64 * bin) < hi)
        .map(|(_, v)| v)
        .sum::<f64>()
        / total
}

#[derive(Clone, Default)]
pub struct Metrics {
    pub lufs: f64,
    pub peak_db: f64,
    pub clipped: usize,
    pub dc: f64,
    pub centroid: f64,
    /// Shares of the energy: under 100 Hz, 100-500, 500-2k, 2-5k, over 5 kHz.
    pub bands: [f64; 5],
    /// A bass's fundamental band (30-100 Hz) out against in, dB (energy share).
    pub low_keep_db: Option<f64>,
    /// The input's share under 100 Hz (a bass DI has most of its energy there).
    pub in_lt100: f64,
    /// Times faster than real time, and the slowest 128-sample block (microseconds).
    pub speed: f64,
    pub worst_block_us: f64,
}

fn measure(x: &[f32], y: &[f32], bass: bool, speed: f64, worst: f64) -> Metrics {
    let spec = spectrum(y);
    let bin = R as f64 / 8192.0;
    let total: f64 = spec.iter().sum::<f64>().max(1e-30);
    let centroid = spec
        .iter()
        .enumerate()
        .map(|(k, v)| k as f64 * bin * v)
        .sum::<f64>()
        / total;
    let peak = y.iter().fold(0.0f32, |m, v| m.max(v.abs())) as f64;
    let low_keep_db = bass.then(|| {
        let sx = spectrum(x);
        10.0 * (band(&spec, 28.0, 100.0) / band(&sx, 28.0, 100.0).max(1e-12))
            .max(1e-9)
            .log10()
    });
    Metrics {
        lufs: lufs(y),
        peak_db: 20.0 * peak.max(1e-9).log10(),
        clipped: y.iter().filter(|v| v.abs() >= 0.975).count(),
        dc: y.iter().map(|&v| v as f64).sum::<f64>() / y.len().max(1) as f64,
        centroid,
        bands: [
            band(&spec, 0.0, 100.0),
            band(&spec, 100.0, 500.0),
            band(&spec, 500.0, 2000.0),
            band(&spec, 2000.0, 5000.0),
            band(&spec, 5000.0, 24000.0),
        ],
        low_keep_db,
        in_lt100: band(&spectrum(x), 0.0, 100.0),
        speed,
        worst_block_us: worst,
    }
}

/// Render as the audio thread does (128 at a time), timing it.
fn render_timed(board: &Board, x: &[f32]) -> (Vec<f32>, f64, f64) {
    let mut p = board::make_board(board, RATE);
    let mut y = x.to_vec();
    let mut worst = 0.0f64;
    let t0 = Instant::now();
    for c in y.chunks_mut(128) {
        let t = Instant::now();
        p.process(c);
        worst = worst.max(t.elapsed().as_secs_f64() * 1e6);
    }
    let took = t0.elapsed().as_secs_f64().max(1e-9);
    (y, x.len() as f64 / R as f64 / took, worst)
}

/// What a chain sounds like against what it should: the automatic flags (the verdicts are read beside them).
fn flags(m: &Metrics, cab: bool, bass: bool) -> Vec<String> {
    let mut f = Vec::new();
    if m.peak_db > -0.3 || m.clipped > 0 {
        f.push(format!(
            "hot: peak {:+.1} dBFS, {} samples at the limiter",
            m.peak_db, m.clipped
        ));
    }
    if m.dc.abs() > 1e-3 {
        f.push(format!("DC {:.4}", m.dc));
    }
    if cab && !bass && m.bands[4] > 0.03 {
        f.push(format!("fizz: {:.1}% over 5 kHz", 100.0 * m.bands[4]));
    }
    if cab && bass && m.bands[4] > 0.02 {
        f.push(format!("bright for bass: {:.1}% over 5 kHz", 100.0 * m.bands[4]));
    }
    if let Some(k) = m.low_keep_db
        && k < -6.0
    {
        f.push(format!("thin: the fundamental band {k:+.1} dB"));
    }
    // mud: more of the energy under 100 Hz than came in (a bass DI is mostly there already)
    if (bass && m.bands[0] > (m.in_lt100 + 0.08).min(0.97))
        || (!bass && (m.bands[0] > 0.35 || m.bands[0] > m.in_lt100 + 0.15))
    {
        f.push(format!(
            "muddy: {:.0}% under 100 Hz (in: {:.0}%)",
            100.0 * m.bands[0],
            100.0 * m.in_lt100
        ));
    }
    // (on a machine busy with builds the numbers halve; a capture runs at about 7x on an idle one)
    if m.speed < 10.0 {
        f.push(format!("slow: {:.0}x real time", m.speed));
    }
    f
}

/// A short word on a chain from its numbers (what to listen for), the flags first.
fn verdict(m: &Metrics, fl: &[String], bass: bool, preset: bool, clean: Option<f64>) -> String {
    let mut v: Vec<String> = Vec::new();
    if !fl.iter().all(|f| f.starts_with("slow")) {
        v.push("check".into());
    }
    if bass {
        v.push(
            match m.centroid {
                c if c < 90.0 => "round, dark",
                c if c < 150.0 => "punchy",
                _ => "growly, mid-forward",
            }
            .into(),
        );
        if let Some(k) = m.low_keep_db {
            v.push(if k > -1.0 {
                "fundamental solid".into()
            } else {
                format!("fundamental {k:+.1} dB")
            });
        }
    } else {
        v.push(
            match m.centroid {
                c if c < 500.0 => "warm",
                c if c < 1000.0 => "balanced",
                c if c < 1800.0 => "bright, cutting",
                _ => "very bright",
            }
            .into(),
        );
        if m.bands[4] < 0.01 {
            v.push("top rolled off as a cab should".into());
        }
    }
    if preset && let Some(c) = clean {
        let d = m.lufs - c;
        v.push(if d.abs() <= 1.0 {
            "on Clean's level".into()
        } else {
            format!("{d:+.1} LU from Clean")
        });
    }
    v.join("; ")
}

// -- the chains -----------------------------------------------------------------------------------------------------

pub(crate) struct Chain {
    pub(crate) group: &'static str,
    pub(crate) name: String,
    pub(crate) what: String,
    pub(crate) board: Board,
    pub(crate) input: &'static str,
    cab: bool,
}

fn b(id: &str) -> Block {
    Block::of(id).unwrap_or_else(|| panic!("model {id}"))
}

fn describe(board: &Board) -> String {
    let mut parts: Vec<String> = board
        .blocks
        .iter()
        .filter(|b| b.on)
        .map(|b| {
            let d = b.def();
            let diff: Vec<String> = d
                .knobs
                .iter()
                .enumerate()
                .filter(|(i, k)| (b.v[*i] - k.def).abs() > 1e-3)
                .map(|(i, k)| format!("{} {}", k.key, k.text(b.v[i])))
                .collect();
            let name = if d.file {
                format!("{} #{}", d.id, b.asset)
            } else {
                d.id.to_string()
            };
            if diff.is_empty() {
                name
            } else {
                format!("{name} ({})", diff.join(", "))
            }
        })
        .collect();
    if board.level_db.abs() > 0.01 {
        parts.push(format!("level {:+.1} dB", board.level_db));
    }
    parts.join(" > ")
}

fn chain(group: &'static str, name: impl Into<String>, board: Board, input: &'static str) -> Chain {
    let cab = board.blocks.iter().any(|b| b.def().kind == Kind::Cab);
    Chain {
        group,
        name: name.into(),
        what: describe(&board),
        board,
        input,
        cab,
    }
}

fn preset_input(name: &str) -> &'static str {
    if name.starts_with("Bass Slap") {
        "bass_slap"
    } else if name.starts_with("Bass Metal")
        || name.starts_with("Bass Rock")
        || name.starts_with("Bass Octave")
    {
        "bass_pick"
    } else if name.starts_with("Bass") {
        "bass_finger"
    } else {
        match name {
            "Clean" | "Ambient" | "Acoustic" | "Blues" | "Jangle" | "Dotted 8th" => "guitar_chords",
            "Lead" | "Fuzz" => "guitar_lick",
            _ => "guitar_chug",
        }
    }
}

pub(crate) fn chains() -> Vec<Chain> {
    let mut out = Vec::new();
    for (name, board) in board::presets() {
        out.push(chain("presets", name, board, preset_input(name)));
    }
    // each amp, clean and driven, through its own cab and another
    for (_, m) in catalog::of_kind(Kind::Amp) {
        let bass = m.id.contains("bass") || m.id == "amp_b15";
        let gain_key = if m.knobs.iter().any(|k| k.key == "gain") {
            "gain"
        } else {
            "volume"
        };
        let cabs: [&str; 2] = match m.id {
            "amp_clean" | "amp_chime" => ["cab_2x12", "cab_4x12"],
            "amp_tweed" => ["cab_1x12", "cab_2x12"],
            "amp_b15" => ["cab_1x15", "cab_8x10"],
            id if id.contains("bass") => ["cab_8x10", "cab_4x10"],
            _ => ["cab_4x12", "cab_1x12"],
        };
        for (label, g) in [("clean", 3.0), ("driven", 7.0)] {
            for cab in cabs {
                let board = Board::new(vec![b(m.id).with(gain_key, g), b(cab)]);
                let input = match (bass, label) {
                    (true, "clean") => "bass_finger",
                    (true, _) => "bass_pick",
                    (false, "clean") => "guitar_chords",
                    _ => "guitar_chug",
                };
                out.push(chain("amps", format!("{}-{label}-{cab}", m.id), board, input));
            }
        }
    }
    // every drive and fuzz into an amp (guitar into the Plexi at 4; the bass ones into the SVT)
    for kind in [Kind::Drive, Kind::Fuzz] {
        for (_, m) in catalog::of_kind(kind) {
            let bass = m.id.starts_with("bass_");
            let board = if bass {
                Board::new(vec![b(m.id), b("amp_bass").with("gain", 3.0), b("cab_8x10")])
            } else {
                Board::new(vec![b(m.id), b("amp_crunch").with("gain", 4.0), b("cab_4x12")])
            };
            let input = if bass { "bass_pick" } else { "guitar_chug" };
            out.push(chain("drives", format!("{}-into-amp", m.id), board, input));
            if !bass {
                // the same pedal set up for bass (Blend 50%, Clean low 120 Hz) into the SVT
                let board = Board::new(vec![
                    b(m.id).for_bass(),
                    b("amp_bass").with("gain", 3.0),
                    b("cab_8x10"),
                ]);
                out.push(chain("drives", format!("{}-for-bass", m.id), board, "bass_pick"));
            }
        }
    }
    // bass chains: the grinder's blend, the low B through every bass amp, the octaver, the filter, chorus, DI mix
    for blend in [0.0, 50.0, 100.0] {
        let board = Board::new(vec![
            b("bass_grind").with("drive", 7.0).with("blend", blend),
            b("amp_bass").with("gain", 2.0),
            b("cab_4x10").with("di", 50.0),
        ]);
        out.push(chain(
            "bass",
            format!("grind-blend-{blend:.0}"),
            board,
            "bass_pick",
        ));
    }
    for amp in ["amp_bass", "amp_b15", "amp_bass_ss", "amp_bass_tube"] {
        let board = Board::new(vec![b(amp), b("cab_8x10").with("di", 30.0)]);
        out.push(chain("bass", format!("low-b-{amp}"), board, "bass_low_b"));
    }
    out.push(chain(
        "bass",
        "octaver-oct2",
        Board::new(vec![
            b("octaver").with("sub", 7.0).with("sub2", 4.0).with("dry", 7.0),
            b("amp_bass"),
            b("cab_8x10"),
        ]),
        "bass_finger",
    ));
    out.push(chain(
        "bass",
        "env-filter-lo-down",
        Board::new(vec![
            b("env_filter")
                .with("range", 1.0)
                .with("direction", 1.0)
                .with("mix", 70.0),
            b("amp_bass_ss"),
            b("cab_4x10"),
        ]),
        "bass_finger",
    ));
    for cut in [0.0, 3.0] {
        out.push(chain(
            "bass",
            format!("chorus-lowcut-{cut:.0}"),
            Board::new(vec![
                b("amp_bass"),
                b("cab_8x10"),
                b("chorus").with("low_cut", cut).with("mix", 60.0),
            ]),
            "bass_finger",
        ));
    }
    for di in [0.0, 50.0, 100.0] {
        out.push(chain(
            "bass",
            format!("4x10-di-{di:.0}-horn"),
            Board::new(vec![
                b("amp_bass_ss"),
                b("cab_4x10").with("di", di).with("horn", 5.0),
            ]),
            "bass_slap",
        ));
    }
    out
}

/// The owner's captures, registered from the library in place: (amp chains, the files' names).
pub(crate) fn nam_chains(dir: &Path) -> anyhow::Result<Vec<Chain>> {
    let text = std::fs::read_to_string(dir.join("captures.json"))?;
    let v: serde_json::Value = serde_json::from_str(&text)?;
    let items = v
        .get("items")
        .and_then(|i| i.as_array())
        .cloned()
        .unwrap_or_default();
    let get_s =
        |it: &serde_json::Value, k: &str| it.get(k).and_then(|x| x.as_str()).unwrap_or("").to_string();
    let get_f = |it: &serde_json::Value, k: &str| it.get(k).and_then(|x| x.as_f64());
    let id_of = |it: &serde_json::Value| it.get("id").and_then(|x| x.as_u64()).unwrap_or(0) as u32;
    let mut registered = std::collections::HashSet::new();
    let mut register = |it: &serde_json::Value| -> bool {
        let id = id_of(it);
        if registered.contains(&id) {
            return true;
        }
        let path = dir.join(get_s(it, "file"));
        nam::set_trim(id, get_f(it, "trim").unwrap_or(0.0) as f32);
        if let (Some(t), Some(g)) = (it.get("tone_id").and_then(|x| x.as_u64()), get_f(it, "gain")) {
            let k = match get_s(it, "kind").as_str() {
                "nam" => 0,
                "pedal" => 1,
                _ => 2,
            };
            nam::set_ladder(id, t * 4 + k, g as f32);
        }
        let ok = match get_s(it, "kind").as_str() {
            "ir" => std::fs::read(&path)
                .ok()
                .is_some_and(|b| nam::register_ir(id, &b).is_ok()),
            _ => std::fs::read_to_string(&path)
                .ok()
                .is_some_and(|j| nam::register_nam(id, &j).is_ok()),
        };
        if ok {
            registered.insert(id);
        }
        ok
    };
    let mut out = Vec::new();
    for (skin, input) in [
        ("amp_clean", "guitar_chords"),
        ("amp_chime", "guitar_chords"),
        ("amp_crunch", "guitar_chug"),
        ("amp_lead", "guitar_chug"),
        ("amp_modern", "guitar_chug"),
        ("amp_bass", "bass_pick"),
    ] {
        let Some(it) = items
            .iter()
            .find(|it| get_s(it, "kind") == "nam" && get_s(it, "skin") == skin)
        else {
            continue;
        };
        // its whole ladder, so the Gain knob has somewhere to go
        let tone = it.get("tone_id").and_then(|x| x.as_u64());
        for sib in items.iter().filter(|s| {
            get_s(s, "kind") == "nam" && tone.is_some() && s.get("tone_id").and_then(|x| x.as_u64()) == tone
        }) {
            register(sib);
        }
        if !register(it) {
            continue;
        }
        let id = id_of(it);
        let with_cab = it.get("cab").and_then(|c| c.as_bool()).unwrap_or(false);
        let cab: Option<Block> = if with_cab {
            None
        } else if let Some(ir) = it
            .get("ir")
            .and_then(|x| x.as_u64())
            .and_then(|ir| items.iter().find(|s| id_of(s) as u64 == ir))
            .filter(|s| register(s))
        {
            let mut c = b("cab_ir");
            c.asset = id_of(ir);
            Some(c)
        } else {
            Some(b(if skin == "amp_bass" {
                "cab_8x10"
            } else {
                "cab_4x12"
            }))
        };
        let name = safe(&get_s(it, "name"));
        let mut amp = b("amp_nam");
        amp.set_asset(id);
        let mk = |amp: Block| {
            let mut v = vec![amp];
            v.extend(cab);
            Board::new(v)
        };
        out.push(chain("nam", format!("{skin}-{name}"), mk(amp), input));
        let mut eq = amp;
        eq.set("bass", -3.0).set("treble", 4.0).set("mid", 2.0);
        out.push(chain("nam", format!("{skin}-{name}-eq"), mk(eq), input));
        let ladder = nam::ladder(id);
        if ladder.len() > 1 {
            for (_, g) in [ladder[0], ladder[ladder.len() - 1]] {
                let mut l = amp;
                l.set("gain", g);
                out.push(chain("nam", format!("{skin}-{name}-ladder-{g:.1}"), mk(l), input));
            }
        }
    }
    // a pedal capture into our amp, alone and blended (a bass DI pedal if there is one)
    for (bass, input, amp, cab) in [
        (false, "guitar_chug", "amp_crunch", "cab_4x12"),
        (true, "bass_pick", "amp_bass", "cab_8x10"),
    ] {
        if let Some(it) = items.iter().find(|it| {
            get_s(it, "kind") == "pedal" && it.get("bass").and_then(|x| x.as_bool()).unwrap_or(false) == bass
        }) && register(it)
        {
            let name = safe(&get_s(it, "name"));
            for blend in [100.0, 50.0] {
                let mut p = b("pedal_nam");
                p.set_asset(id_of(it));
                p.set("blend", blend);
                if bass && blend < 100.0 {
                    p.set("clean_low", 2.0);
                }
                let board = Board::new(vec![p, b(amp).with("gain", 3.0), b(cab)]);
                out.push(chain(
                    "nam",
                    format!("pedal-{name}-blend-{blend:.0}"),
                    board,
                    input,
                ));
            }
        }
    }
    Ok(out)
}

fn safe(s: &str) -> String {
    let t: String = s
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    let mut t = t
        .split('-')
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    t.truncate(40);
    t
}

// -- the stress tests -----------------------------------------------------------------------------------------------

/// Energy away from the harmonics of `f` (and above 40 Hz) against all of it, dB: aliasing and other junk.
fn inharmonic_db(y: &[f32], f: f32) -> f64 {
    use realfft::RealFftPlanner;
    let n = 32768.min(y.len().next_power_of_two() / 2);
    let x = &y[y.len() - n..];
    let mut planner = RealFftPlanner::<f64>::new();
    let fft = planner.plan_fft_forward(n);
    let mut buf: Vec<f64> = x
        .iter()
        .enumerate()
        .map(|(k, &v)| {
            // Blackman-Harris: its sidelobes are far under what we look for
            let a = std::f64::consts::TAU * k as f64 / n as f64;
            v as f64 * (0.35875 - 0.48829 * a.cos() + 0.14128 * (2.0 * a).cos() - 0.01168 * (3.0 * a).cos())
        })
        .collect();
    let mut spec = fft.make_output_vec();
    if fft.process(&mut buf, &mut spec).is_err() {
        return 0.0;
    }
    let bin = R as f64 / n as f64;
    let (mut harm, mut junk) = (0.0f64, 0.0f64);
    for (k, c) in spec.iter().enumerate() {
        let fr = k as f64 * bin;
        if fr < 40.0 {
            continue;
        }
        let h = (fr / f as f64).round();
        let p = c.norm_sqr();
        if h >= 1.0 && (fr - h * f as f64).abs() <= 4.0 * bin {
            harm += p;
        } else {
            junk += p;
        }
    }
    10.0 * (junk / (harm + junk).max(1e-30)).max(1e-15).log10()
}

struct Stress {
    rows: Vec<String>,
}

fn stress(nam_amp: Option<Block>) -> Stress {
    let mut rows = Vec::new();
    // aliasing: high sines through high gain, before the cab (the worst case) and with it (what you hear)
    let mut chains: Vec<(String, Vec<Block>)> = vec![
        ("amp_lead gain 9".into(), vec![b("amp_lead").with("gain", 9.0)]),
        (
            "amp_modern gain 9".into(),
            vec![b("amp_modern").with("gain", 9.0)],
        ),
        (
            "metal > amp_modern 8".into(),
            vec![b("metal").with("gain", 9.0), b("amp_modern").with("gain", 8.0)],
        ),
        (
            "big_pi > amp_clean 5".into(),
            vec![
                b("big_pi").with("sustain", 10.0),
                b("amp_clean").with("gain", 5.0),
            ],
        ),
        (
            "orange > amp_crunch 7".into(),
            vec![b("orange").with("dist", 10.0), b("amp_crunch").with("gain", 7.0)],
        ),
        (
            "bass_grind drive 10".into(),
            vec![b("bass_grind").with("drive", 10.0).with("blend", 100.0)],
        ),
        (
            "amp_bass_ss gain 10".into(),
            vec![b("amp_bass_ss").with("gain", 10.0)],
        ),
        (
            "amp_bass_tube gain 10".into(),
            vec![b("amp_bass_tube").with("gain", 10.0)],
        ),
    ];
    if let Some(a) = nam_amp {
        chains.push((format!("NAM capture #{}", a.asset), vec![a]));
    }
    rows.push("| chain | 1.5 kHz | 3.3 kHz | 5.1 kHz | 5.1 kHz with a 4x12 |".into());
    rows.push("|---|---|---|---|---|".into());
    for (name, blocks) in &chains {
        let mut cells = Vec::new();
        for f in [1500.0f32, 3300.0, 5100.0] {
            let x: Vec<f32> = (0..RATE as usize)
                .map(|i| 0.25 * (std::f64::consts::TAU * f as f64 * i as f64 / R as f64).sin() as f32)
                .collect();
            let y = board::render(&Board::new(blocks.clone()), RATE, &x);
            cells.push(format!("{:.0} dB", inharmonic_db(&y, f)));
        }
        let mut with_cab = blocks.clone();
        with_cab.push(b("cab_4x12"));
        let x: Vec<f32> = (0..RATE as usize)
            .map(|i| 0.25 * (std::f64::consts::TAU * 5100.0 * i as f64 / R as f64).sin() as f32)
            .collect();
        let y = board::render(&Board::new(with_cab), RATE, &x);
        cells.push(format!("{:.0} dB", inharmonic_db(&y, 5100.0)));
        rows.push(format!("| {name} | {} |", cells.join(" | ")));
    }
    // the noise floor: -90 dBFS of hiss into every starting board; and silence after a note (denormals)
    rows.push(String::new());
    rows.push(
        "| board | idle hiss out (dBFS RMS, from -90 in) | time per block: notes / silence after (us) | tail peak |"
            .into(),
    );
    rows.push("|---|---|---|---|".into());
    let hiss = testsig::hiss(RATE as usize * 2, 3.162e-5, 7);
    let guitar = crate::board_cmd::dry(&crate::board_cmd::chug(), false);
    for (name, board) in board::presets() {
        let y = board::render(&board, RATE, &hiss);
        let tail = &y[RATE as usize..];
        let rms = (tail.iter().map(|v| v * v).sum::<f32>() / tail.len() as f32).sqrt();
        let mut p = board::make_board(&board, RATE);
        let mut x = guitar.clone();
        let t = Instant::now();
        for c in x.chunks_mut(128) {
            p.process(c);
        }
        let per_note = t.elapsed().as_secs_f64() * 1e6 / (guitar.len() / 128) as f64;
        let mut silence = vec![0.0f32; RATE as usize * 10];
        let t = Instant::now();
        for c in silence.chunks_mut(128) {
            p.process(c);
        }
        let per_quiet = t.elapsed().as_secs_f64() * 1e6 / (silence.len() / 128) as f64;
        let tail_peak = silence[silence.len() - RATE as usize..]
            .iter()
            .fold(0.0f32, |m, v| m.max(v.abs()));
        rows.push(format!(
            "| {name} | {:.1} | {per_note:.1} / {per_quiet:.1} | {:.1e} |",
            20.0 * rms.max(1e-12).log10(),
            tail_peak
        ));
    }
    Stress { rows }
}

// -- the run --------------------------------------------------------------------------------------------------------

pub fn run(out: &Path, only: Option<&str>, captures: Option<PathBuf>) -> anyhow::Result<()> {
    std::fs::create_dir_all(out)?;
    let mats = material();
    for m in &mats {
        crate::board_cmd::write(&out.join("dry").join(format!("{}.wav", m.name)), &m.x)?;
    }
    let mut list = chains();
    let cap_dir = captures.or_else(|| {
        std::env::var_os("APPDATA").map(|a| PathBuf::from(a).join("quickRS").join("data").join("captures"))
    });
    let mut nam_amp = None;
    if let Some(dir) = cap_dir.filter(|d| d.join("captures.json").exists()) {
        match nam_chains(&dir) {
            Ok(n) => {
                nam_amp = n
                    .iter()
                    .find(|c| c.name.starts_with("amp_lead"))
                    .and_then(|c| c.board.blocks.first().copied());
                eprintln!("{} NAM chains from {}", n.len(), dir.display());
                list.extend(n);
            }
            Err(e) => eprintln!("no NAM chains: {e}"),
        }
    }
    if let Some(o) = only {
        list.retain(|c| c.name.contains(o) || c.group.contains(o));
    }
    let mut csv = String::from(
        "group,name,input,lufs,peak_db,clipped,dc,centroid,lt100,b100_500,b500_2k,b2k_5k,gt5k,low_keep_db,speed,worst_us,flags,chain\n",
    );
    let mut md = String::new();
    let mut groups: Vec<&str> = Vec::new();
    let mut rows: std::collections::HashMap<&str, Vec<String>> = Default::default();
    let mut preset_lufs = Vec::new();
    for c in &list {
        let m = mats.iter().find(|m| m.name == c.input).expect("material");
        let (y, speed, worst) = render_timed(&c.board, &m.x);
        let me = measure(&m.x, &y, m.bass, speed, worst);
        let fl = flags(&me, c.cab, m.bass);
        let file = format!("{}/{}.wav", c.group, c.name);
        crate::board_cmd::write(&out.join(&file), &y)?;
        if c.group == "presets" {
            preset_lufs.push((c.name.clone(), me.lufs, m.bass));
        }
        let clean = preset_lufs.iter().find(|p| p.0 == "Clean").map(|p| p.1);
        let word = verdict(&me, &fl, m.bass, c.group == "presets", clean);
        let _ = writeln!(
            csv,
            "{},{},{},{:.2},{:.2},{},{:.5},{:.0},{:.3},{:.3},{:.3},{:.3},{:.4},{},{:.0},{:.0},\"{}\",\"{}\"",
            c.group,
            c.name,
            c.input,
            me.lufs,
            me.peak_db,
            me.clipped,
            me.dc,
            me.centroid,
            me.bands[0],
            me.bands[1],
            me.bands[2],
            me.bands[3],
            me.bands[4],
            me.low_keep_db.map(|v| format!("{v:.1}")).unwrap_or_default(),
            me.speed,
            me.worst_block_us,
            fl.join("; "),
            c.what
        );
        if !groups.contains(&c.group) {
            groups.push(c.group);
        }
        rows.entry(c.group).or_default().push(format!(
            "| [{}]({}) | {} | {} | {:.1} | {:+.1} | {:.0} | {:.1}% | {} | {} | {} |",
            c.name,
            file.replace(' ', "%20"),
            c.what,
            c.input,
            me.lufs,
            me.peak_db,
            me.centroid,
            100.0 * me.bands[4],
            me.low_keep_db
                .map(|v| format!("{v:+.1}"))
                .unwrap_or_else(|| "-".into()),
            if fl.is_empty() { "ok".into() } else { fl.join("; ") },
            word
        ));
        println!(
            "{:<44} {:6.1} LUFS {:+5.1} dBFS {}",
            file,
            me.lufs,
            me.peak_db,
            fl.join("; ")
        );
    }
    std::fs::write(out.join("metrics.csv"), &csv)?;
    let st = stress(nam_amp);
    // the index: the summary is written by hand above the tables (the run keeps what's between the markers)
    let _ = writeln!(
        md,
        "<!-- generated by `quickrs-cli board listen`: tables below are rewritten on each run -->"
    );
    if !preset_lufs.is_empty() {
        let g: Vec<f64> = preset_lufs.iter().filter(|p| !p.2).map(|p| p.1).collect();
        let bs: Vec<f64> = preset_lufs.iter().filter(|p| p.2).map(|p| p.1).collect();
        let spread = |v: &[f64]| {
            v.iter().cloned().fold(f64::MIN, f64::max) - v.iter().cloned().fold(f64::MAX, f64::min)
        };
        let _ = writeln!(
            md,
            "\nStarting boards' loudness: guitar {:.1} LU spread over {} boards, bass {:.1} LU over {} (synthetic DI, peaks at -10 dBFS).\n",
            spread(&g),
            g.len(),
            spread(&bs),
            bs.len()
        );
    }
    for g in groups {
        let _ = writeln!(md, "\n## {g}\n");
        let _ = writeln!(
            md,
            "| file | chain | input | LUFS | peak dBFS | centroid Hz | >5 kHz | fundamental kept (dB) | measured | verdict |"
        );
        let _ = writeln!(md, "|---|---|---|---|---|---|---|---|---|---|");
        for r in rows.get(g).into_iter().flatten() {
            let _ = writeln!(md, "{r}");
        }
    }
    let _ = writeln!(
        md,
        "\n## Stress tests\n\nAliasing: the energy away from the harmonics of a 0.25-peak sine, against all of it.\n"
    );
    for r in &st.rows {
        let _ = writeln!(md, "{r}");
    }
    // INDEX.md: what is written above the marker (the listening notes, by hand) is kept; the tables are new
    let index = out.join("INDEX.md");
    let marker = "<!-- generated by `quickrs-cli board listen`";
    let head = std::fs::read_to_string(&index)
        .ok()
        .and_then(|t| t.find(marker).map(|i| t[..i].to_string()))
        .unwrap_or_else(|| {
            "# Listening set

"
            .into()
        });
    std::fs::write(&index, format!("{head}{md}"))?;
    println!("-> {}", out.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loudness_is_bs1770s() {
        // a 1 kHz sine at -20 dBFS peak reads about -23.0 LUFS (the K-weighting is ~+0.7 dB at 1 kHz, -3 dB for RMS)
        let x: Vec<f32> = (0..RATE as usize * 5)
            .map(|i| 0.1 * (std::f64::consts::TAU * 1000.0 * i as f64 / R as f64).sin() as f32)
            .collect();
        let l = lufs(&x);
        assert!((l - (-23.0)).abs() < 0.3, "{l}");
        assert!(lufs(&vec![0.0; 48000]) < -90.0);
    }

    #[test]
    fn a_clean_sine_has_no_inharmonic_energy() {
        let x: Vec<f32> = (0..RATE as usize)
            .map(|i| 0.25 * (std::f64::consts::TAU * 3300.0 * i as f64 / R as f64).sin() as f32)
            .collect();
        assert!(inharmonic_db(&x, 3300.0) < -90.0);
    }
}
