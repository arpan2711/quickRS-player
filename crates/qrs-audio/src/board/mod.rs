//! The pedalboard (PLAN.md Phase 14, D26): a tone made of blocks (pedals, an amp, a cab, racks) in an order you
//! choose, each switched on or off and turned with its own knobs. It runs as one `ToneProcessor` on the input
//! side; knob turns, switches, new blocks, removed blocks and moves are made in place on the audio thread
//! (`BoardCmd`), so delay and reverb tails keep ringing while you change things. A new board altogether (a
//! preset, a song's tone change) is a whole-tone swap, crossfaded by the input side.
//!
//! Bypass fades each block in and out over 10 ms. Delays and reverbs are "wet only" blocks: the dry signal passes
//! by them and they add their echoes, so switching one off stops what goes in and lets its tail ring out.

pub mod catalog;
pub mod dsp;
pub mod level;
pub mod loudness;
pub mod picture;
pub mod rs_gear;

pub mod amp;
mod cab;
mod delay;
mod drive;
mod dynamics;
mod eq;
mod filter;
mod modfx;
pub mod nam;
mod reverb;

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_bass;
#[cfg(test)]
mod tests_levels;

use std::sync::atomic::{AtomicU32, Ordering};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

pub use catalog::{Face, Kind, KnobDef, MODELS, ModelDef, Unit};

/// Blocks on a board at most.
pub const MAX_BLOCKS: usize = 12;
/// Knobs on a model at most.
/// (16 since P37.2: the SVT with its Ultra switches has 13.)
pub const MAX_KNOBS: usize = 16;

/// One model running on the audio thread. Everything it needs is allocated by its constructor.
pub trait Pedal: Send {
    /// Process mono samples in place.
    fn process(&mut self, x: &mut [f32]);
    /// Process with the board's input (the DI) beside it, for a pedal that listens to it (the keyed gate, P41.1);
    /// the rest ignore it.
    fn process_keyed(&mut self, x: &mut [f32], key: &[f32]) {
        let _ = key;
        self.process(x);
    }
    /// The knob values, in the model's knob order and units (the catalogue's `KnobDef`s). Called whenever any
    /// of them changes; the pedal glides to them.
    fn set(&mut self, v: &[f32; MAX_KNOBS]);
    fn reset(&mut self) {}
    /// Delays and reverbs: `process` leaves only the wet signal (the board adds the dry), so switching it off can
    /// let the tail ring out.
    fn wet_only(&self) -> bool {
        false
    }
}

static NEXT_UID: AtomicU32 = AtomicU32::new(1);

/// The song's tempo (beats a minute, as f32 bits; 0: none), for the delays' Sync (P37.2). Set by the app when a
/// song opens or its tempo is known; read on the audio thread (an atomic load, no lock).
static TEMPO: AtomicU32 = AtomicU32::new(0);

pub fn set_tempo(bpm: f32) {
    let b = if bpm.is_finite() && bpm > 20.0 {
        bpm.min(400.0)
    } else {
        0.0
    };
    TEMPO.store(b.to_bits(), Ordering::Relaxed);
}

pub fn tempo() -> f32 {
    f32::from_bits(TEMPO.load(Ordering::Relaxed))
}

/// A song's tempo from its beat times (the median beat, beats a minute; 0 with too few beats).
pub fn tempo_of_beats(beats: &[f64]) -> f32 {
    let mut d: Vec<f64> = beats
        .windows(2)
        .map(|w| w[1] - w[0])
        .filter(|x| *x > 0.15 && *x < 2.0)
        .collect();
    if d.len() < 2 {
        return 0.0;
    }
    d.sort_by(f64::total_cmp);
    (60.0 / d[d.len() / 2]) as f32
}

/// A fresh id for a block (it follows the block through edits, so the running board can find it).
pub fn new_uid() -> u32 {
    NEXT_UID.fetch_add(1, Ordering::Relaxed)
}

/// One block: a model, on or off, and its knob values (in the model's knob order and units).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Block {
    pub model: usize,
    pub on: bool,
    pub v: [f32; MAX_KNOBS],
    pub uid: u32,
    /// The file a model runs (a NAM capture's or an IR's id in the captures library, `nam`); 0 for the rest.
    pub asset: u32,
}

impl Block {
    /// A model at its default settings.
    pub fn new(model: usize) -> Block {
        Block {
            model,
            on: true,
            v: MODELS[model].defaults(),
            uid: new_uid(),
            asset: 0,
        }
    }

    pub fn of(id: &str) -> Option<Block> {
        catalog::find(id).map(Block::new)
    }

    pub fn def(&self) -> &'static ModelDef {
        &MODELS[self.model]
    }

    pub fn get(&self, key: &str) -> Option<f32> {
        self.def().knob_index(key).map(|i| self.v[i])
    }

    /// Set a knob by key (clamped to its range); unknown keys are ignored.
    pub fn set(&mut self, key: &str, v: f32) -> &mut Block {
        if let Some(i) = self.def().knob_index(key) {
            self.v[i] = self.def().knobs[i].clamp(v);
        }
        self
    }

    pub fn with(mut self, key: &str, v: f32) -> Block {
        self.set(key, v);
        self
    }

    /// The same model with another one's knobs carried over where the keys match (changing model in a kind).
    pub fn changed_to(&self, model: usize) -> Block {
        let mut b = Block {
            model,
            on: self.on,
            v: MODELS[model].defaults(),
            uid: self.uid,
            asset: if MODELS[model].file { self.asset } else { 0 },
        };
        for (i, k) in self.def().knobs.iter().enumerate() {
            if let Some(j) = MODELS[model].knob_index(k.key)
                && MODELS[model].knobs[j].unit == k.unit
            {
                b.v[j] = MODELS[model].knobs[j].clamp(self.v[i]);
            }
        }
        b
    }

    pub fn to_json(&self) -> Value {
        let mut knobs = Map::new();
        for (i, k) in self.def().knobs.iter().enumerate() {
            knobs.insert(
                k.key.into(),
                Value::from((self.v[i] as f64 * 1000.0).round() / 1000.0),
            );
        }
        let mut m = Map::new();
        m.insert("model".into(), Value::from(self.def().id));
        m.insert("on".into(), Value::from(self.on));
        m.insert("knobs".into(), Value::Object(knobs));
        if self.asset != 0 {
            m.insert("asset".into(), Value::from(self.asset));
        }
        Value::Object(m)
    }

    /// A block from JSON (None for a model this version doesn't know).
    pub fn from_json(v: &Value) -> Option<Block> {
        let mut b = Block::of(v.get("model")?.as_str()?)?;
        b.on = v.get("on").and_then(Value::as_bool).unwrap_or(true);
        if b.def().file {
            b.asset = v.get("asset").and_then(Value::as_u64).unwrap_or(0) as u32;
        }
        if let Some(knobs) = v.get("knobs").and_then(Value::as_object) {
            for (k, x) in knobs {
                if let Some(f) = x.as_f64() {
                    b.set(k, f as f32);
                }
            }
            // (P41.1, D162: a gate saved before it had Range and Key turned all the way down and listened to what
            // reached it; it keeps doing so, wherever it sits)
            if b.def().id == "gate" && !knobs.contains_key("range") {
                b.set("range", -90.0);
                b.set("key", 1.0);
            }
        }
        Some(b)
    }
}

/// A board: its blocks in signal order and an output level.
#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize)]
#[serde(from = "Value", into = "Value")]
pub struct Board {
    pub blocks: Vec<Block>,
    pub level_db: f32,
}

impl From<Value> for Board {
    fn from(v: Value) -> Board {
        Board::from_json(&v)
    }
}

impl From<Board> for Value {
    fn from(b: Board) -> Value {
        b.to_json()
    }
}

impl Board {
    pub fn new(blocks: Vec<Block>) -> Board {
        Board {
            blocks,
            level_db: 0.0,
        }
    }

    pub fn to_json(&self) -> Value {
        let mut m = Map::new();
        m.insert(
            "blocks".into(),
            Value::Array(self.blocks.iter().map(Block::to_json).collect()),
        );
        m.insert(
            "level".into(),
            Value::from((self.level_db as f64 * 100.0).round() / 100.0),
        );
        Value::Object(m)
    }

    pub fn from_json(v: &Value) -> Board {
        let blocks = v
            .get("blocks")
            .and_then(Value::as_array)
            .map(|a| a.iter().filter_map(Block::from_json).take(MAX_BLOCKS).collect())
            .unwrap_or_default();
        Board {
            blocks,
            level_db: v
                .get("level")
                .and_then(Value::as_f64)
                .map(|l| (l as f32).clamp(-24.0, 12.0))
                .unwrap_or(0.0),
        }
    }

    /// The same board with fresh block ids (a copy that edits apart from its original).
    pub fn renewed(&self) -> Board {
        let mut b = self.clone();
        for blk in &mut b.blocks {
            blk.uid = new_uid();
        }
        b
    }

    pub fn index_of(&self, uid: u32) -> Option<usize> {
        self.blocks.iter().position(|b| b.uid == uid)
    }

    /// The first block of a kind.
    pub fn first(&self, kind: Kind) -> Option<usize> {
        self.blocks.iter().position(|b| b.def().kind == kind)
    }
}

// -- the running board ----------------------------------------------------------------------------------------------

/// Changes to a running board, made on the audio thread without allocating. Blocks are found by uid.
pub enum BoardCmd {
    Knob {
        uid: u32,
        knob: usize,
        value: f32,
    },
    Bypass {
        uid: u32,
        on: bool,
    },
    /// A new block (its pedal built off the audio thread), added at the end; an `Order` puts it in place.
    Insert {
        block: Block,
        pedal: Box<dyn Pedal>,
    },
    /// The block's model changed: the new pedal takes its place.
    Replace {
        block: Block,
        pedal: Box<dyn Pedal>,
    },
    /// Fade the block out (a delay or reverb lets its tail ring first), then drop it.
    Remove {
        uid: u32,
    },
    /// The blocks in this order (the first `n` uids).
    Order {
        uids: [u32; MAX_BLOCKS],
        n: usize,
    },
    Level(f32),
}

impl BoardCmd {
    /// The pedal a command carries (to be freed when the tone isn't a board).
    pub fn into_pedal(self) -> Option<Box<dyn Pedal>> {
        match self {
            BoardCmd::Insert { pedal, .. } | BoardCmd::Replace { pedal, .. } => Some(pedal),
            _ => None,
        }
    }
}

/// The file a block plays: its asset, or for a capture with its Gain turned, the unit's capture at that gain (the gain
/// ladder, P37.2).
pub fn effective_asset(b: &Block) -> u32 {
    match b.def().id {
        "amp_nam" | "pedal_nam" => nam::ladder_pick(b.asset, b.get("gain").unwrap_or(-1.0)),
        _ => b.asset,
    }
}

impl Block {
    /// Put a capture or IR on this block (from the library): its Gain knob goes to the capture's own gain when the
    /// library knows it (so the ladder starts where the capture is), else "as loaded".
    pub fn set_asset(&mut self, asset: u32) -> &mut Block {
        self.asset = asset;
        if self.def().knob_index("gain").is_some() && self.def().file {
            self.set("gain", nam::ladder_gain(asset).unwrap_or(-1.0));
        }
        self
    }

    /// The same block as a bass player sets it (P37.2): a drive's or fuzz's Blend at half and its Clean low at
    /// 120 Hz (the Darkglass and Parallax default), a chorus's Low cut at 200 Hz. Other blocks unchanged.
    pub fn for_bass(mut self) -> Block {
        let kind = self.def().kind;
        if matches!(kind, Kind::Drive | Kind::Fuzz) {
            if self.def().knob_index("blend").is_some() && self.get("blend") == Some(100.0) {
                self.set("blend", 50.0);
            }
            self.set("clean_low", 2.0);
        }
        if self.def().id == "chorus" {
            self.set("low_cut", 3.0);
        }
        self
    }
}

/// The pedal for a block, set to its knobs (allocates: off the audio thread).
pub fn make_pedal(b: &Block, rate: u32) -> Box<dyn Pedal> {
    let mut p = match b.def().id {
        "amp_nam" => nam::make_amp(effective_asset(b), rate as f32),
        "pedal_nam" => nam::make_pedal_capture(effective_asset(b), rate as f32),
        "cab_ir" => nam::make_cab(b.asset, rate as f32),
        _ => (b.def().make)(rate as f32),
    };
    p.set(&b.v);
    p.reset();
    p
}

/// The commands that turn the running `old` board into `new`, or None when so much changed that a fresh board is
/// better (fewer than half of `new`'s blocks were on `old`). Builds new blocks' pedals (allocates).
pub fn diff(old: &Board, new: &Board, rate: u32) -> Option<Vec<BoardCmd>> {
    let kept = new
        .blocks
        .iter()
        .filter(|b| old.index_of(b.uid).is_some())
        .count();
    if !new.blocks.is_empty() && kept * 2 < new.blocks.len() {
        return None;
    }
    let mut cmds = Vec::new();
    for b in &old.blocks {
        if new.index_of(b.uid).is_none() {
            cmds.push(BoardCmd::Remove { uid: b.uid });
        }
    }
    for b in &new.blocks {
        match old.index_of(b.uid).map(|i| old.blocks[i]) {
            None => cmds.push(BoardCmd::Insert {
                block: *b,
                pedal: make_pedal(b, rate),
            }),
            Some(o)
                if o.model != b.model
                    || o.asset != b.asset
                    || (b.def().file && effective_asset(&o) != effective_asset(b)) =>
            {
                cmds.push(BoardCmd::Replace {
                    block: *b,
                    pedal: make_pedal(b, rate),
                })
            }
            Some(o) => {
                for k in 0..b.def().knobs.len() {
                    if o.v[k] != b.v[k] {
                        cmds.push(BoardCmd::Knob {
                            uid: b.uid,
                            knob: k,
                            value: b.v[k],
                        });
                    }
                }
                if o.on != b.on {
                    cmds.push(BoardCmd::Bypass { uid: b.uid, on: b.on });
                }
            }
        }
    }
    let order_same = old
        .blocks
        .iter()
        .map(|b| b.uid)
        .filter(|u| new.index_of(*u).is_some())
        .eq(new
            .blocks
            .iter()
            .map(|b| b.uid)
            .filter(|u| old.index_of(*u).is_some()));
    let added = new.blocks.iter().any(|b| old.index_of(b.uid).is_none());
    if !order_same || added {
        let mut uids = [0u32; MAX_BLOCKS];
        for (o, b) in uids.iter_mut().zip(&new.blocks) {
            *o = b.uid;
        }
        cmds.push(BoardCmd::Order {
            uids,
            n: new.blocks.len().min(MAX_BLOCKS),
        });
    }
    if old.level_db != new.level_db {
        cmds.push(BoardCmd::Level(new.level_db));
    }
    Some(cmds)
}

struct Slot {
    uid: u32,
    pedal: Box<dyn Pedal>,
    v: [f32; MAX_KNOBS],
    on: bool,
    /// Being removed: fading out, then dropped.
    dying: bool,
    /// The fade between dry (0) and the block (1).
    g: f32,
    wet_only: bool,
    /// Samples the switched-off tail has been silent.
    quiet: usize,
    /// The pedal a `Replace` took over from, still playing while `xf` crossfades from it (0) to the new one (1):
    /// a model swap or a capture's gain ladder step glides instead of dipping to the dry guitar (P37.2).
    old: Option<Box<dyn Pedal>>,
    xf: f32,
}

/// Samples processed at a time (longer blocks are split).
const CHUNK: usize = 512;
const FADE_MS: f32 = 10.0;
/// A replaced pedal's crossfade into the new one.
const XFADE_MS: f32 = 25.0;

/// A board running on the audio thread.
pub struct BoardProcessor {
    rate: f32,
    slots: Vec<Slot>,
    dry: Vec<f32>,
    wet: Vec<f32>,
    /// The board's input this chunk (the DI): a keyed gate listens to it wherever it sits (P41.1).
    key: Vec<f32>,
    /// The outgoing pedal's output during a crossfade.
    xbuf: Vec<f32>,
    /// Pedals dropped while processing, for the trash.
    dropped: Vec<Box<dyn Pedal>>,
    level: f32,
    level_target: f32,
    /// A switched-off tail stops being processed after this many silent samples.
    tail_hold: usize,
}

impl BoardProcessor {
    pub fn new(board: &Board, rate: u32) -> BoardProcessor {
        let mut slots = Vec::with_capacity(MAX_BLOCKS * 2);
        for b in board.blocks.iter().take(MAX_BLOCKS) {
            let pedal = make_pedal(b, rate);
            slots.push(Slot {
                uid: b.uid,
                wet_only: pedal.wet_only(),
                pedal,
                v: b.v,
                on: b.on,
                dying: false,
                g: if b.on { 1.0 } else { 0.0 },
                quiet: usize::MAX / 2,
                old: None,
                xf: 1.0,
            });
        }
        let level = db_gain(board.level_db);
        BoardProcessor {
            rate: rate as f32,
            slots,
            dry: vec![0.0; CHUNK],
            wet: vec![0.0; CHUNK],
            key: vec![0.0; CHUNK],
            xbuf: vec![0.0; CHUNK],
            dropped: Vec::with_capacity(MAX_BLOCKS * 4),
            level,
            level_target: level,
            tail_hold: (rate as usize) / 2,
        }
    }

    fn slot(&mut self, uid: u32) -> Option<&mut Slot> {
        self.slots.iter_mut().find(|s| s.uid == uid && !s.dying)
    }

    fn process_chunk(&mut self, x: &mut [f32]) {
        let n = x.len();
        let step = 1.0 / (FADE_MS * 1e-3 * self.rate);
        let xstep = 1.0 / (XFADE_MS * 1e-3 * self.rate);
        self.key[..n].copy_from_slice(x);
        let (dry, wet, xb, key) = (
            &mut self.dry[..n],
            &mut self.wet[..n],
            &mut self.xbuf[..n],
            &self.key[..n],
        );
        for s in self.slots.iter_mut() {
            let target = if s.on && !s.dying { 1.0 } else { 0.0 };
            if let Some(old) = s.old.as_mut() {
                // a crossfade from the replaced pedal (both in series blocks: `Replace` starts one only then)
                xb.copy_from_slice(x);
                old.process_keyed(xb, key);
                dry.copy_from_slice(x);
                s.pedal.process_keyed(x, key);
                let (mut xf, mut g) = (s.xf, s.g);
                for ((v, &d), &o) in x.iter_mut().zip(dry.iter()).zip(xb.iter()) {
                    xf = approach(xf, 1.0, xstep);
                    g = approach(g, target, step);
                    let y = o + (*v - o) * xf;
                    *v = d + (y - d) * g;
                }
                s.xf = xf;
                s.g = g;
                if xf >= 1.0
                    && let Some(p) = s.old.take()
                {
                    if self.dropped.len() < self.dropped.capacity() {
                        self.dropped.push(p);
                    } else {
                        std::mem::forget(p);
                    }
                }
                continue;
            }
            if s.wet_only {
                if s.g == 0.0 && target == 0.0 && s.quiet >= self.tail_hold {
                    continue;
                }
                let mut g = s.g;
                for (w, &v) in wet.iter_mut().zip(x.iter()) {
                    g = approach(g, target, step);
                    *w = v * g;
                }
                s.g = g;
                s.pedal.process(wet);
                let mut peak = 0.0f32;
                for (v, &w) in x.iter_mut().zip(wet.iter()) {
                    *v += w;
                    peak = peak.max(w.abs());
                }
                if target == 0.0 && s.g == 0.0 && peak < 1e-5 {
                    s.quiet = s.quiet.saturating_add(n);
                } else {
                    s.quiet = 0;
                }
            } else {
                if s.g == 0.0 && target == 0.0 {
                    continue;
                }
                if s.g == 1.0 && target == 1.0 {
                    s.pedal.process_keyed(x, key);
                    continue;
                }
                dry.copy_from_slice(x);
                s.pedal.process_keyed(x, key);
                let mut g = s.g;
                for (v, &d) in x.iter_mut().zip(dry.iter()) {
                    g = approach(g, target, step);
                    *v = d + (*v - d) * g;
                }
                s.g = g;
            }
        }
        // drop the blocks that have faded out (and whose tails have died)
        let mut i = 0;
        while i < self.slots.len() {
            let s = &self.slots[i];
            if s.dying && s.g == 0.0 && (!s.wet_only || s.quiet >= self.tail_hold) {
                let s = self.slots.remove(i);
                for p in std::iter::once(s.pedal).chain(s.old) {
                    if self.dropped.len() < self.dropped.capacity() {
                        self.dropped.push(p);
                    } else {
                        std::mem::forget(p);
                    }
                }
            } else {
                i += 1;
            }
        }
        let (l0, l1) = (self.level, self.level_target);
        let dl = (l1 - l0) / n.max(1) as f32;
        for (i, v) in x.iter_mut().enumerate() {
            *v = dsp::soft_limit(*v * (l0 + dl * i as f32));
        }
        self.level = l1;
    }
}

#[inline]
fn approach(g: f32, target: f32, step: f32) -> f32 {
    if g < target {
        (g + step).min(target)
    } else if g > target {
        (g - step).max(target)
    } else {
        g
    }
}

fn db_gain(db: f32) -> f32 {
    10f32.powf(db / 20.0)
}

impl crate::rt::ToneProcessor for BoardProcessor {
    fn process(&mut self, x: &mut [f32]) {
        for c in x.chunks_mut(CHUNK) {
            self.process_chunk(c);
        }
    }

    fn reset(&mut self) {
        for s in &mut self.slots {
            s.pedal.reset();
            if let Some(o) = s.old.as_mut() {
                o.reset();
            }
        }
    }

    fn board(&mut self, cmd: BoardCmd) -> Option<Box<dyn Pedal>> {
        match cmd {
            BoardCmd::Knob { uid, knob, value } => {
                if let Some(s) = self.slot(uid)
                    && knob < MAX_KNOBS
                {
                    s.v[knob] = value;
                    let v = s.v;
                    s.pedal.set(&v);
                }
                None
            }
            BoardCmd::Bypass { uid, on } => {
                if let Some(s) = self.slot(uid) {
                    s.on = on;
                    if on {
                        s.quiet = 0;
                    }
                }
                None
            }
            BoardCmd::Insert { block, pedal } => {
                if self.slots.len() >= self.slots.capacity() {
                    return Some(pedal);
                }
                self.slots.push(Slot {
                    uid: block.uid,
                    wet_only: pedal.wet_only(),
                    pedal,
                    v: block.v,
                    on: block.on,
                    dying: false,
                    g: 0.0,
                    quiet: 0,
                    old: None,
                    xf: 1.0,
                });
                None
            }
            BoardCmd::Replace { block, pedal } => match self.slot(block.uid) {
                Some(s) => {
                    // a series block that is playing crossfades from the old pedal; otherwise it fades in from dry
                    let glide = !s.wet_only && !pedal.wet_only() && s.on && block.on && s.g > 0.0;
                    s.v = block.v;
                    s.on = block.on;
                    s.wet_only = pedal.wet_only();
                    s.quiet = 0;
                    if glide {
                        let cur = std::mem::replace(&mut s.pedal, pedal);
                        s.xf = 0.0;
                        // (a second step while one is gliding: the oldest goes to the trash)
                        s.old.replace(cur)
                    } else {
                        s.g = 0.0;
                        Some(std::mem::replace(&mut s.pedal, pedal))
                    }
                }
                None => Some(pedal),
            },
            BoardCmd::Remove { uid } => {
                if let Some(s) = self.slot(uid) {
                    s.dying = true;
                    s.uid = 0;
                }
                None
            }
            BoardCmd::Order { uids, n } => {
                // live slots in the given order, dying ones after them
                let mut at = 0;
                for &u in uids.iter().take(n.min(MAX_BLOCKS)) {
                    if let Some(j) =
                        (at..self.slots.len()).find(|&j| self.slots[j].uid == u && !self.slots[j].dying)
                    {
                        self.slots[at..=j].rotate_right(1);
                        at += 1;
                    }
                }
                None
            }
            BoardCmd::Level(db) => {
                self.level_target = db_gain(db.clamp(-24.0, 12.0));
                None
            }
        }
    }

    fn trash(&mut self) -> Option<Box<dyn Pedal>> {
        self.dropped.pop()
    }
}

/// The processor for a board (allocates: off the audio thread).
pub fn make_board(board: &Board, rate: u32) -> Box<dyn crate::rt::ToneProcessor> {
    Box::new(BoardProcessor::new(board, rate))
}

/// Render `x` (mono) through a board offline, in blocks of 128 like the audio thread.
pub fn render(board: &Board, rate: u32, x: &[f32]) -> Vec<f32> {
    use crate::rt::ToneProcessor;
    let mut p = BoardProcessor::new(board, rate);
    let mut y = x.to_vec();
    for c in y.chunks_mut(128) {
        p.process(c);
    }
    y
}

/// How much louder a board makes an idle input's hiss (dB, from -90 dBFS of white noise). Crunch is 19 to 24 (a JCM800
/// at gain 3.5, our Crunch and Blues), as a real amp hisses; over about 28 the hiss between notes is loud and the
/// board wants a gate (a JCM800 from gain 6, the Mesa-style amp 57, a Tube Screamer into the lead amp 50, fuzz 47).
/// Renders 0.4 s, off the audio thread.
pub fn hiss_gain_db(board: &Board) -> f32 {
    hiss_gain_at(board)
}

/// Over this much hiss gain (`hiss_gain_db`) a board's idle noise is plainly audible (a guitar's single-coil buzz
/// comes out near -50 dBFS, `quickrs-cli board idle`): song tones get a gentle gate and the room offers one (P38.2).
pub const GATE_HISS_DB: f32 = 20.0;

// -- the gate's threshold from the input's noise (P41.1, D163) -------------------------------------------------------

/// The input's noise floor after the input boost (dBFS, as f32 bits; NaN: not measured), from Calibrate or the room's
/// Learn noise. Set by the app (`calibrate::apply`); read when a gate is made.
static NOISE_FLOOR: AtomicU32 = AtomicU32::new(0x7fc0_0000);

/// A gate opens this far over the input's noise floor (and closes 6 dB under that, its hysteresis).
pub const GATE_OVER_FLOOR_DB: f32 = 8.0;

pub fn set_noise_floor(db: Option<f32>) {
    let v = db
        .filter(|d| d.is_finite())
        .map(|d| d.clamp(-120.0, 0.0))
        .unwrap_or(f32::NAN);
    NOISE_FLOOR.store(v.to_bits(), Ordering::Relaxed);
}

/// The input's noise floor after the input boost (dBFS), when measured.
pub fn noise_floor() -> Option<f32> {
    let v = f32::from_bits(NOISE_FLOOR.load(Ordering::Relaxed));
    v.is_finite().then_some(v)
}

/// A gate's threshold (dB): the measured noise floor plus `GATE_OVER_FLOOR_DB`; unmeasured, -62 for a gentle gate (a
/// crunch) and -54 for a firm one (high gain), as before. Within the knob's -80..-30.
pub fn gate_threshold(firm: bool) -> f32 {
    match noise_floor() {
        Some(f) => (f + GATE_OVER_FLOOR_DB).clamp(-80.0, -30.0),
        None if firm => -54.0,
        None => -62.0,
    }
}

/// A gate as a song tone or the room adds one (P41.1, D162): keyed on the input, its threshold from the noise floor; a
/// gentle one (a crunch: turns the hiss down 30 dB, a slow 150 ms) or a firm one (high gain: 60 dB, 120 ms).
pub fn new_gate(firm: bool) -> Block {
    Block::of("gate")
        .expect("gate")
        .with("threshold", gate_threshold(firm))
        .with("release", if firm { 120.0 } else { 150.0 })
        .with("range", if firm { -60.0 } else { -30.0 })
        .with("key", 0.0)
}

/// Where a new gate goes on a board (P41.1, D162): right after the amp (the first one on, else the first), else after
/// the last drive or fuzz, else first. Keyed on the input, it turns the amp's hiss and sustain down together there,
/// and comes before the delays and reverbs, whose tails ring on.
pub fn gate_slot(b: &Board) -> usize {
    let amp = b
        .blocks
        .iter()
        .position(|x| x.on && x.def().kind == Kind::Amp)
        .or_else(|| b.first(Kind::Amp));
    if let Some(i) = amp {
        // (a capture's own model of ours, off, follows it: the gate goes after both)
        let mut j = i + 1;
        while j < b.blocks.len() && b.blocks[j].def().kind == Kind::Amp && !b.blocks[j].on {
            j += 1;
        }
        return j;
    }
    b.blocks
        .iter()
        .rposition(|x| matches!(x.def().kind, Kind::Drive | Kind::Fuzz))
        .map(|i| i + 1)
        .unwrap_or(0)
}

fn hiss_gain_at(board: &Board) -> f32 {
    const RATE: u32 = 48000;
    const RMS: f32 = 3.162e-5; // -90 dBFS
    let n = RATE as usize * 2 / 5;
    let mut seed = 0x2545_f491u32;
    let x: Vec<f32> = (0..n)
        .map(|_| {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            // uniform on [-a, a] has an RMS of a / sqrt(3)
            (seed as f32 / u32::MAX as f32 * 2.0 - 1.0) * RMS * 3f32.sqrt()
        })
        .collect();
    let y = render(board, RATE, &x);
    let tail = &y[n / 4..];
    let rms = (tail.iter().map(|v| v * v).sum::<f32>() / tail.len() as f32).sqrt();
    20.0 * (rms.max(1e-9) / RMS).log10()
}

// -- starting boards ------------------------------------------------------------------------------------------------

/// The boards the tile offers before you've saved any of your own: (name, board).
pub fn presets() -> Vec<(&'static str, Board)> {
    let b = |id: &str| Block::of(id).expect("catalogue model");
    let mut boards = vec![
        (
            "Clean",
            Board::new(vec![
                b("squeeze").with("sustain", 3.5).with("level", 5.5),
                b("amp_clean").with("gain", 3.5).with("treble", 6.5),
                b("cab_2x12").with("mic", 5.0),
                b("spring").with("dwell", 5.0).with("mix", 22.0),
            ]),
        ),
        (
            "Crunch",
            Board::new(vec![
                // (P38.2: a gentle gate: the Plexi at 6 raises an idle guitar's buzz ~20 dB; P41.1: after the amp,
                // keyed on the input, turning the hiss down 30 dB)
                b("amp_crunch").with("gain", 6.0).with("mid", 6.5),
                b("gate").with("threshold", -62.0).with("release", 150.0),
                b("cab_4x12").with("mic", 4.0),
                b("room").with("mix", 14.0),
            ]),
        ),
        (
            "Lead",
            Board::new(vec![
                b("green").with("drive", 3.0).with("level", 7.0),
                b("amp_lead").with("gain", 7.0),
                b("gate")
                    .with("threshold", -54.0)
                    .with("release", 120.0)
                    .with("range", -60.0),
                b("cab_4x12"),
                b("analog_delay").with("time", 380.0).with("mix", 22.0),
                b("plate").with("mix", 16.0),
            ]),
        ),
        (
            "Metal",
            Board::new(vec![
                b("green").with("drive", 0.5).with("tone", 6.0).with("level", 8.0),
                b("amp_modern").with("gain", 7.5),
                b("gate").with("threshold", -50.0).with("range", -60.0),
                b("cab_4x12").with("mic", 3.5),
            ]),
        ),
        (
            "Blues",
            Board::new(vec![
                b("blues").with("gain", 4.0),
                b("amp_tweed").with("volume", 6.0),
                b("gate").with("threshold", -62.0).with("release", 150.0),
                b("cab_1x12"),
                b("spring").with("mix", 20.0),
            ]),
        ),
        (
            "Ambient",
            Board::new(vec![
                b("squeeze").with("sustain", 4.0),
                b("amp_clean").with("gain", 3.0),
                b("cab_2x12"),
                b("mod_delay")
                    .with("time", 480.0)
                    .with("repeats", 5.5)
                    .with("mix", 35.0),
                b("hall").with("time", 5.0).with("mix", 35.0),
            ]),
        ),
        (
            "Fuzz",
            Board::new(vec![
                b("big_pi"),
                b("amp_clean").with("gain", 5.0),
                b("gate")
                    .with("threshold", -54.0)
                    .with("release", 120.0)
                    .with("range", -60.0),
                b("cab_4x12"),
            ]),
        ),
        (
            "Acoustic",
            Board::new(vec![
                b("acoustic"),
                b("studio_comp").with("threshold", -22.0).with("ratio", 3.0),
                b("room").with("mix", 18.0),
            ]),
        ),
        (
            "Bass",
            Board::new(vec![b("bass_comp"), b("amp_bass"), b("cab_8x10")]),
        ),
        // -- P37.2: two more guitar starters, then the bass set (BASS-GEAR.md section 4); names starting "Bass"
        // are bass boards (`preset_is_bass`)
        (
            "Jangle",
            Board::new(vec![
                b("amp_chime")
                    .with("gain", 4.0)
                    .with("treble", 6.5)
                    .with("cut", 3.5),
                b("cab_2x12").with("mic", 5.0),
                b("room").with("mix", 14.0),
            ]),
        ),
        (
            "Dotted 8th",
            Board::new(vec![
                b("squeeze").with("sustain", 3.0),
                b("amp_clean").with("gain", 3.5).with("treble", 6.5),
                b("cab_2x12"),
                // a dotted eighth at the song's tempo (380 ms without one)
                b("digital_delay")
                    .with("sync", 2.0)
                    .with("repeats", 4.5)
                    .with("mix", 32.0),
                b("plate").with("mix", 14.0),
            ]),
        ),
        (
            "Bass Motown",
            Board::new(vec![
                b("opto_comp").with("peak", 5.0),
                b("amp_b15")
                    .with("volume", 3.5)
                    .with("treble", 4.0)
                    .with("bass", 6.0),
                b("cab_1x15").with("mic_type", 1.0).with("di", 30.0),
            ]),
        ),
        (
            "Bass Rock pick",
            Board::new(vec![
                b("bass_comp").with("compress", 5.0),
                b("amp_bass")
                    .with("gain", 5.0)
                    .with("mid", 6.5)
                    .with("freq", 800.0),
                b("cab_8x10").with("di", 40.0),
            ]),
        ),
        (
            "Bass Slap",
            Board::new(vec![
                b("studio_comp")
                    .with("threshold", -24.0)
                    .with("ratio", 4.0)
                    .with("attack", 2.0)
                    .with("release", 90.0)
                    .with("makeup", 6.0),
                b("amp_bass_ss")
                    .with("contour", 1.0)
                    .with("bass", 3.0)
                    .with("treble", 4.0),
                b("cab_4x10").with("horn", 6.0).with("di", 60.0),
            ]),
        ),
        (
            "Bass Metal grind",
            Board::new(vec![
                b("gate").with("threshold", -58.0).with("range", -90.0),
                b("bass_grind")
                    .with("drive", 6.0)
                    .with("blend", 50.0)
                    .with("level", 7.0)
                    .with("grunt", 0.0)
                    .with("clean_low", 2.0),
                b("amp_bass").with("gain", 2.0),
                b("cab_4x10").with("di", 50.0),
            ]),
        ),
        (
            "Bass Funk",
            Board::new(vec![
                b("env_filter")
                    .with("range", 1.0)
                    .with("mix", 70.0)
                    .with("sens", 6.0)
                    .with("boost", 6.0),
                b("amp_bass_ss").with("mid", -3.0),
                b("cab_4x10").with("di", 40.0),
            ]),
        ),
        (
            "Bass Dub",
            Board::new(vec![
                b("opto_comp").with("peak", 6.0),
                b("amp_bass").with("bass", 8.0).with("treble", 2.0),
                b("cab_1x15").with("high_cut", 3000.0),
                b("tape_echo").with("mix", 18.0).with("repeats", 4.5),
            ]),
        ),
        (
            "Bass Octave fuzz",
            Board::new(vec![
                b("gate").with("threshold", -60.0).with("range", -90.0),
                b("octaver").with("sub", 6.0).with("dry", 7.0),
                b("bass_muff").with("blend", 60.0),
                b("amp_bass").with("gain", 3.0),
                b("cab_8x10").with("di", 30.0),
            ]),
        ),
    ];
    // output levels matched to Clean by loudness (BS.1770, K-weighted): the owner's guitar through the Scarlett,
    // two takes rendered through every board (2026-10-15); Crunch had come out 3.6 LU under Clean, Blues 1.5 over
    for (name, board) in &mut boards {
        board.level_db = match *name {
            "Crunch" | "Acoustic" => 3.5,
            "Ambient" => 3.0,
            "Lead" => 2.0,
            "Bass" => 1.5,
            "Blues" => -1.5,
            "Metal" | "Fuzz" => -1.0,
            // P37.2's boards, matched to Clean by BS.1770 on the synthetic DI (`quickrs-cli board listen`: the
            // guitar ones on the strummed chords, the bass ones on the bass lines; all peak at -10 dBFS)
            "Jangle" => 2.5,
            "Dotted 8th" => 0.5,
            "Bass Motown" => -1.5,
            "Bass Rock pick" => 2.5,
            "Bass Slap" => 5.0,
            "Bass Metal grind" => 3.0,
            "Bass Funk" => 6.0,
            "Bass Dub" => -2.0,
            "Bass Octave fuzz" => -1.0,
            _ => 0.0,
        };
    }
    boards
}

/// A starting board for a bass (P37.2): the app offers these first on a bass input.
pub fn preset_is_bass(name: &str) -> bool {
    name.starts_with("Bass")
}
