//! The amp room's sounds and scenes (PLAN.md P22.2, D47). New code (no Python source).
//!
//! - userdata.json `room_rigs`: the sounds you saved, a list of {name, tags, color, slot, based_on, board, scenes,
//!   saved}. (`rigs` is a song's tones as you changed them, D26.) Kept with the playlists, so it goes where they go.
//! - config.json `tone_board` is the rig in the room (your own board: the player's MINE is the same board), and
//!   `room` = {rig, based_on, scene, scenes} what the room had when you left it: the saved sound it is (its name),
//!   what it was based on, the scene playing and the four scenes. The last rig reopens when you come back.
//!
//! A scene keeps which blocks are on and their knobs (as a board's JSON). Scenes of the same chain switch in place:
//! the settings go onto the rig block by block, the uids stay, and the engine changes knobs and bypasses (no gap). A
//! sound loaded into a scene brings its own chain; switching to it is a new board (the engine's 30 ms crossfade).

use qrs_audio::board::{self, Board, Kind, MODELS};
use qrs_library::{Config, UserData};
use serde_json::{Map, Value, json};

pub const SCENES: usize = 4;
/// A scene's name when the user hasn't named it (B9: fixed "Clean / Crunch / Lead / Ambient" promised sounds that
/// weren't there).
pub fn scene_label(names: &[String; SCENES], i: usize) -> String {
    match names.get(i).map(|s| s.trim()) {
        Some(n) if !n.is_empty() => n.to_string(),
        _ => format!("Scene {}", i + 1),
    }
}
pub const SCENE_COLOURS: [[u8; 3]; SCENES] =
    [[74, 139, 224], [62, 196, 109], [240, 100, 90], [138, 107, 224]];
/// A sound's colour choices (the save sheet's swatches).
pub const COLOURS: [[u8; 3]; 8] = [
    [74, 139, 224],
    [62, 196, 109],
    [242, 181, 60],
    [240, 100, 90],
    [138, 107, 224],
    [63, 182, 201],
    [232, 106, 166],
    [232, 226, 210],
];
/// The tags the save sheet offers: (group, tags).
pub const TAGS: [(&str, &[&str]); 2] = [
    (
        "GENRE",
        &["Indie", "Garage rock", "Rock", "Punk", "Blues", "Pop", "Metal"],
    ),
    (
        "CHARACTER",
        &["Clean", "Crunch", "Lead", "Ambient", "Fuzz", "Bass"],
    ),
];

/// A saved sound.
#[derive(Clone, Debug, PartialEq)]
pub struct Rig {
    pub name: String,
    pub tags: Vec<String>,
    /// Index into `COLOURS`.
    pub color: usize,
    /// Favourite slot 1-4 (0: none).
    pub slot: u8,
    pub based_on: String,
    pub board: Board,
    pub scenes: [Option<Board>; SCENES],
    pub saved: f64,
}

impl Rig {
    pub fn to_json(&self) -> Value {
        json!({
            "name": self.name,
            "tags": self.tags,
            "color": self.color,
            "slot": self.slot,
            "based_on": self.based_on,
            "board": self.board.to_json(),
            "scenes": scenes_json(&self.scenes),
            "saved": self.saved,
        })
    }

    pub fn from_json(v: &Value) -> Option<Rig> {
        let name = v.get("name")?.as_str()?.trim().to_string();
        if name.is_empty() {
            return None;
        }
        Some(Rig {
            name,
            tags: v
                .get("tags")
                .and_then(Value::as_array)
                .map(|a| a.iter().filter_map(|t| t.as_str().map(String::from)).collect())
                .unwrap_or_default(),
            color: v
                .get("color")
                .and_then(Value::as_u64)
                .map(|c| (c as usize).min(COLOURS.len() - 1))
                .unwrap_or(0),
            slot: v
                .get("slot")
                .and_then(Value::as_u64)
                .map(|s| s.min(4) as u8)
                .unwrap_or(0),
            based_on: v
                .get("based_on")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
            board: v.get("board").map(Board::from_json).unwrap_or_default(),
            scenes: scenes_from(v.get("scenes")),
            saved: v.get("saved").and_then(Value::as_f64).unwrap_or(0.0),
        })
    }

    /// Its blocks in order, as names: "Squeeze › Mean Green › Perspex 100".
    pub fn chain(&self) -> String {
        chain_text(&self.board)
    }
}

pub fn chain_text(b: &Board) -> String {
    b.blocks
        .iter()
        .map(|x| {
            if x.on {
                x.def().name.to_string()
            } else {
                format!("{} (off)", x.def().name)
            }
        })
        .collect::<Vec<_>>()
        .join(" \u{203a} ")
}

pub fn scenes_json(s: &[Option<Board>; SCENES]) -> Value {
    Value::Array(
        s.iter()
            .map(|b| b.as_ref().map(Board::to_json).unwrap_or(Value::Null))
            .collect(),
    )
}

pub fn scenes_from(v: Option<&Value>) -> [Option<Board>; SCENES] {
    let mut out: [Option<Board>; SCENES] = Default::default();
    if let Some(a) = v.and_then(Value::as_array) {
        for (i, x) in a.iter().take(SCENES).enumerate() {
            if x.is_object() {
                out[i] = Some(Board::from_json(x));
            }
        }
    }
    out
}

/// The sounds you saved (in the order saved; ones this version can't read are kept in the file as they were).
pub fn saved(ud: &UserData) -> Vec<Rig> {
    ud.other
        .get("room_rigs")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Rig::from_json).collect())
        .unwrap_or_default()
}

/// Keep a sound (replacing one of the same name, any case); a favourite slot it takes is freed on the others.
pub fn store(ud: &mut UserData, rig: &Rig) {
    let mut list: Vec<Value> = ud
        .other
        .get("room_rigs")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let same = |v: &Value| {
        v.get("name")
            .and_then(Value::as_str)
            .is_some_and(|n| n.trim().eq_ignore_ascii_case(rig.name.trim()))
    };
    if rig.slot > 0 {
        for v in list.iter_mut() {
            if !same(v) && v.get("slot").and_then(Value::as_u64) == Some(rig.slot as u64) {
                v["slot"] = json!(0);
            }
        }
    }
    match list.iter().position(same) {
        Some(i) => list[i] = rig.to_json(),
        None => list.push(rig.to_json()),
    }
    ud.other.insert("room_rigs".into(), Value::Array(list));
    ud.save();
}

pub fn delete(ud: &mut UserData, name: &str) {
    if let Some(a) = ud.other.get_mut("room_rigs").and_then(Value::as_array_mut) {
        a.retain(|v| {
            !v.get("name")
                .and_then(Value::as_str)
                .is_some_and(|n| n.eq_ignore_ascii_case(name))
        });
        ud.save();
    }
}

/// A sound matches a search: every word is in its name, tags, what it's based on or its chain (the Sounds tab's
/// test; `words` lower case).
pub fn matches(r: &Rig, words: &[&str]) -> bool {
    let hay = format!("{} {} {} {}", r.name, r.tags.join(" "), r.based_on, r.chain()).to_lowercase();
    words.iter().all(|w| hay.contains(w))
}

/// Your sounds as a list shows them: the favourite slots 1-4 first (in slot order), then the rest in the order
/// saved; only those matching `query` (see `matches`). Indices into `list`.
pub fn listed(list: &[Rig], query: &str) -> Vec<usize> {
    let q = query.trim().to_lowercase();
    let words: Vec<&str> = q.split_whitespace().collect();
    let mut out: Vec<usize> = (0..list.len()).filter(|&i| matches(&list[i], &words)).collect();
    // (a stable sort: slot 0 after 1-4, the saved order kept within)
    out.sort_by_key(|&i| match list[i].slot {
        0 => u8::MAX,
        s => s,
    });
    out
}

/// What a saved sound plays when it's put on: its board, its first scene's settings on it when it has one (as the
/// room loads a sound).
pub fn sound_board(r: &Rig) -> Board {
    match &r.scenes[0] {
        Some(s) => scene_board(&r.board, s),
        None => r.board.clone(),
    }
}

/// The room's state with a saved sound put on (at its first scene).
pub fn state_for(r: &Rig) -> RoomState {
    RoomState {
        rig: Some(r.name.clone()),
        based_on: r.based_on.clone(),
        scene: 0,
        scenes: r.scenes.clone(),
        names: Default::default(),
    }
}

/// A name not taken yet ("Glass clean", "Glass clean 2", ...).
pub fn fresh_name(list: &[Rig], base: &str) -> String {
    let taken = |n: &str| list.iter().any(|r| r.name.eq_ignore_ascii_case(n));
    if !taken(base) {
        return base.to_string();
    }
    (2..)
        .map(|i| format!("{base} {i}"))
        .find(|n| !taken(n))
        .unwrap_or_else(|| base.to_string())
}

/// Two boards sound the same: the same blocks, on or off, knobs and level (uids aside).
pub fn same_sound(a: &Board, b: &Board) -> bool {
    a.to_json() == b.to_json()
}

/// The room as you left it (config.json `room`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RoomState {
    /// The saved sound the rig is (its name), if any.
    pub rig: Option<String>,
    pub based_on: String,
    /// The scene playing (0-3).
    pub scene: usize,
    pub scenes: [Option<Board>; SCENES],
    /// The scenes' names as the user gave them ("" = not named: "Scene 2").
    pub names: [String; SCENES],
}

impl RoomState {
    pub fn load(cfg: &Config) -> RoomState {
        let Some(v) = cfg.get("room") else {
            return RoomState::default();
        };
        RoomState {
            rig: v
                .get("rig")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
                .map(String::from),
            based_on: v
                .get("based_on")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
            scene: v
                .get("scene")
                .and_then(Value::as_u64)
                .map(|s| (s as usize).min(SCENES - 1))
                .unwrap_or(0),
            scenes: scenes_from(v.get("scenes")),
            names: {
                let mut n: [String; SCENES] = Default::default();
                if let Some(a) = v.get("scene_names").and_then(Value::as_array) {
                    for (i, x) in a.iter().take(SCENES).enumerate() {
                        n[i] = x.as_str().unwrap_or("").chars().take(24).collect();
                    }
                }
                n
            },
        }
    }

    pub fn save(&self, cfg: &mut Config) {
        let mut m = Map::new();
        m.insert("rig".into(), json!(self.rig.clone().unwrap_or_default()));
        m.insert("based_on".into(), json!(self.based_on));
        m.insert("scene".into(), json!(self.scene));
        m.insert("scenes".into(), scenes_json(&self.scenes));
        m.insert("scene_names".into(), json!(self.names.to_vec()));
        cfg.set("room", Value::Object(m));
    }
}

/// The same models with the same captures in the same order (a scene of this chain: switching to it is in place). A
/// different capture on a block is another chain: the engine crossfades to it (B9).
pub fn same_chain(a: &Board, b: &Board) -> bool {
    a.blocks.len() == b.blocks.len()
        && a.blocks
            .iter()
            .zip(&b.blocks)
            .all(|(x, y)| x.model == y.model && x.asset == y.asset)
}

/// What playing scene `scene` makes of the rig `rig`: the same chain, its settings put on in place (the rig's
/// uids: the engine changes knobs and bypasses, no gap); another chain (a sound loaded into the scene), that board
/// (the engine crossfades over 30 ms).
pub fn scene_board(rig: &Board, scene: &Board) -> Board {
    if same_chain(rig, scene) {
        apply_scene(rig, scene)
    } else {
        scene.renewed()
    }
}

/// A scene's on/off and knobs on the rig: a block takes the scene's settings from the block at the same place when
/// it's the same model (else the first unused one of that model); blocks the scene doesn't have keep theirs. The
/// uids, the chain and the level stay the rig's.
pub fn apply_scene(rig: &Board, scene: &Board) -> Board {
    let mut out = rig.clone();
    let mut used = vec![false; scene.blocks.len()];
    for (i, b) in out.blocks.iter_mut().enumerate() {
        let same = |s: &board::Block| s.model == b.model && s.asset == b.asset;
        let at = if scene.blocks.get(i).is_some_and(same) && !used[i] {
            Some(i)
        } else {
            (0..scene.blocks.len()).find(|&j| !used[j] && same(&scene.blocks[j]))
        };
        if let Some(j) = at {
            used[j] = true;
            b.on = scene.blocks[j].on;
            b.v = scene.blocks[j].v;
        }
    }
    out
}

/// A small, fast random source (xorshift), seeded from the clock: Shuffle and Randomise need no more.
pub struct Dice(u64);

impl Dice {
    pub fn new(seed: u64) -> Dice {
        Dice(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }

    pub fn from_clock() -> Dice {
        let t = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(7);
        Dice::new(t)
    }

    pub fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    /// 0..1
    pub fn unit(&mut self) -> f32 {
        (self.next() >> 40) as f32 / (1u64 << 24) as f32
    }

    pub fn pick(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }

    pub fn chance(&mut self, p: f32) -> bool {
        self.unit() < p
    }
}

/// A block's knobs set at random around the middle (each between 25% and 75% of its travel; switches anywhere).
pub fn randomise(b: &mut board::Block, dice: &mut Dice) {
    let def = b.def();
    for (k, kd) in def.knobs.iter().enumerate() {
        if kd.back {
            continue;
        }
        b.v[k] = match kd.unit {
            board::Unit::Choice(c) => dice.pick(c.len()) as f32,
            _ => kd.from_u(0.25 + 0.5 * dice.unit()),
        };
    }
}

/// A random rig that makes sense: maybe a compressor, a drive or fuzz, an amp and a cab, maybe modulation, a delay,
/// a reverb; knobs around the middle.
pub fn shuffle(dice: &mut Dice) -> Board {
    let pick = |dice: &mut Dice, kind: Kind| -> board::Block {
        let all: Vec<usize> = board::catalog::of_kind(kind).map(|(i, _)| i).collect();
        let mut b = board::Block::new(all[dice.pick(all.len())]);
        randomise(&mut b, dice);
        b
    };
    let mut blocks = Vec::new();
    if dice.chance(0.4) {
        blocks.push(pick(dice, Kind::Dynamics));
    }
    if dice.chance(0.7) {
        let k = if dice.chance(0.25) {
            Kind::Fuzz
        } else {
            Kind::Drive
        };
        blocks.push(pick(dice, k));
    }
    blocks.push(pick(dice, Kind::Amp));
    blocks.push(pick(dice, Kind::Cab));
    if dice.chance(0.35) {
        blocks.push(pick(dice, Kind::Modulation));
    }
    if dice.chance(0.55) {
        blocks.push(pick(dice, Kind::Delay));
    }
    if dice.chance(0.75) {
        blocks.push(pick(dice, Kind::Reverb));
    }
    // a reverb or delay that's subtle: their mix low
    for b in blocks.iter_mut() {
        if matches!(b.def().kind, Kind::Delay | Kind::Reverb)
            && let Some(i) = b.def().knob_index("mix")
        {
            let kd = b.def().knobs[i];
            b.v[i] = kd.from_u(0.15 + 0.2 * dice.unit());
        }
    }
    Board::new(blocks)
}

/// A name for a shuffled rig from its amp and drive ("Perspex 100 + Mean Green").
pub fn shuffle_name(b: &Board) -> String {
    let amp = b.first(Kind::Amp).map(|i| b.blocks[i].def().name);
    let drv = b
        .blocks
        .iter()
        .find(|x| matches!(x.def().kind, Kind::Drive | Kind::Fuzz))
        .map(|x| x.def().name);
    match (amp, drv) {
        (Some(a), Some(d)) => format!("{a} + {d}"),
        (Some(a), None) => a.to_string(),
        _ => "Shuffled".into(),
    }
}

/// The models on the shelf for a kind (the catalogue's order).
pub fn shelf(kind: Kind) -> Vec<usize> {
    (0..MODELS.len())
        .filter(|&i| MODELS[i].family() == kind && (!MODELS[i].file || crate::captures::ON))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_scene_with_another_capture_is_another_chain() {
        let mut a = Board::new(vec![
            board::Block::of("amp_nam").unwrap(),
            board::Block::of("cab_ir").unwrap(),
        ]);
        a.blocks[0].asset = 1;
        let mut b = a.clone();
        assert!(same_chain(&a, &b));
        b.blocks[0].asset = 2;
        // (B9: capture B in a scene used to play capture A)
        assert!(!same_chain(&a, &b));
        let out = scene_board(&a, &b);
        assert_eq!(out.blocks[0].asset, 2);
        assert_eq!(scene_label(&Default::default(), 1), "Scene 2");
        let mut names: [String; SCENES] = Default::default();
        names[0] = "Verse".into();
        assert_eq!(scene_label(&names, 0), "Verse");
    }

    fn ud() -> UserData {
        let dir = std::env::temp_dir().join(format!("qrs-room-{}", board::new_uid()));
        std::fs::create_dir_all(&dir).unwrap();
        UserData::load(dir.join("userdata.json"))
    }

    fn rig(name: &str) -> Rig {
        let (_, b) = board::presets().swap_remove(1);
        let mut scenes: [Option<Board>; SCENES] = Default::default();
        let mut s = b.clone();
        s.blocks[0].on = false;
        scenes[2] = Some(s);
        Rig {
            name: name.into(),
            tags: vec!["Indie".into(), "Lead".into()],
            color: 2,
            slot: 2,
            based_on: "Reptilia · Lead".into(),
            board: b,
            scenes,
            saved: 1_700_000_000.0,
        }
    }

    #[test]
    fn rigs_round_trip_through_userdata() {
        let mut u = ud();
        let r = rig("Reptilia lead, warmer");
        store(&mut u, &r);
        let back = UserData::load(u.path.clone());
        let got = saved(&back);
        assert_eq!(got.len(), 1);
        let g = &got[0];
        assert_eq!(g.name, r.name);
        assert_eq!(g.tags, r.tags);
        assert_eq!((g.color, g.slot), (2, 2));
        assert_eq!(g.based_on, r.based_on);
        assert!(same_sound(&g.board, &r.board));
        assert!(g.scenes[0].is_none() && g.scenes[2].is_some());
        assert!(same_sound(
            g.scenes[2].as_ref().unwrap(),
            r.scenes[2].as_ref().unwrap()
        ));
        // the same name again replaces it; a slot taken moves
        let mut r2 = rig("Other");
        r2.slot = 2;
        store(&mut u, &r2);
        let mut r3 = r.clone();
        r3.tags = vec!["Rock".into()];
        r3.name = "reptilia lead, WARMER".into();
        store(&mut u, &r3);
        let got = saved(&UserData::load(u.path.clone()));
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].tags, vec!["Rock".to_string()]);
        assert_eq!(got[0].slot, 2);
        assert_eq!(got[1].slot, 0, "the other sound gave up slot 2");
        delete(&mut u, "Other");
        assert_eq!(saved(&UserData::load(u.path.clone())).len(), 1);
    }

    #[test]
    fn an_old_userdata_file_loads_and_saves_unchanged() {
        // a userdata.json from before the room: every key it had comes back as it was, and nothing is added
        let dir = std::env::temp_dir().join(format!("qrs-room-old-{}", board::new_uid()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("userdata.json");
        let old = json!({
            "favorites": ["a.psarc|A"],
            "playlists": {"Sunday": ["a.psarc|A"]},
            "scores": {},
            "recent": {"a.psarc|A": 1700000000.0},
            "rigs": {"a.psarc|A": {"lead": {"T1": {"blocks": [], "level": 0.0}}}},
            "something_else": {"kept": true}
        });
        std::fs::write(&path, old.to_string()).unwrap();
        let mut u = UserData::load(&path);
        assert!(saved(&u).is_empty());
        u.save();
        let again: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        for (k, v) in old.as_object().unwrap() {
            assert_eq!(again.get(k), Some(v), "{k} kept as it was");
        }
        assert!(again.get("room_rigs").is_none());
        // a sound saved later leaves the song tones (`rigs`) alone
        store(&mut u, &rig("New"));
        let again: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(again.get("rigs"), old.get("rigs"));
        assert_eq!(again["room_rigs"].as_array().map(Vec::len), Some(1));
    }

    #[test]
    fn a_scene_sets_on_and_knobs_and_keeps_the_rig() {
        let (_, b) = board::presets().swap_remove(1);
        let rig = b.renewed();
        let mut scene = rig.clone();
        scene.blocks[0].on = !scene.blocks[0].on;
        let k = scene.blocks[1].def().knobs.len() - 1;
        scene.blocks[1].v[k] = scene.blocks[1].def().knobs[k].clamp(scene.blocks[1].v[k] + 1.0);
        let scene = scene.renewed(); // (saved and read back: other uids)
        let out = apply_scene(&rig, &scene);
        // the rig's uids: the engine sees knob and bypass changes, not a new board
        let uids: Vec<u32> = out.blocks.iter().map(|x| x.uid).collect();
        assert_eq!(uids, rig.blocks.iter().map(|x| x.uid).collect::<Vec<_>>());
        assert_eq!(out.blocks[0].on, scene.blocks[0].on);
        assert_eq!(out.blocks[1].v, scene.blocks[1].v);
        let cmds = board::diff(&rig, &out, 48_000).expect("an in-place change");
        assert!(
            cmds.iter()
                .all(|c| matches!(c, board::BoardCmd::Knob { .. } | board::BoardCmd::Bypass { .. }))
        );
        // a block the scene doesn't have keeps its settings
        let mut more = rig.clone();
        more.blocks.push(board::Block::of("plate").unwrap());
        let out = apply_scene(&more, &scene);
        assert_eq!(out.blocks.last().unwrap().v, more.blocks.last().unwrap().v);
        // a scene of the same chain plays in place; another chain is its own board
        assert!(same_chain(&rig, &scene));
        assert_eq!(scene_board(&rig, &scene).blocks[0].uid, rig.blocks[0].uid);
        assert!(!same_chain(&more, &scene));
        let other = scene_board(&more, &scene);
        assert!(same_sound(&other, &scene));
    }

    #[test]
    fn sounds_list_slots_first_then_as_saved() {
        let mk = |name: &str, slot: u8| {
            let mut r = rig(name);
            r.slot = slot;
            r.tags = vec![if slot == 0 { "Crunch" } else { "Clean" }.into()];
            r
        };
        let list = vec![
            mk("Strokes rhythm", 0),
            mk("Shimmer pad", 4),
            mk("Glass clean", 1),
            mk("Dry blues", 0),
            mk("Warm lead", 2),
        ];
        assert_eq!(listed(&list, ""), vec![2, 4, 1, 0, 3]);
        // the search: every word, in the name, tags, based on or chain, any case
        assert_eq!(listed(&list, "  GLASS "), vec![2]);
        assert_eq!(listed(&list, "crunch"), vec![0, 3]);
        assert_eq!(listed(&list, "pad clean"), vec![1]);
        assert!(listed(&list, "nothing like it").is_empty());
        let chain_word = list[0].board.blocks[0].def().name.to_lowercase();
        assert_eq!(listed(&list, &chain_word).len(), list.len());
    }

    #[test]
    fn a_sound_put_on_plays_its_first_scene() {
        let r = rig("Lead");
        // no first scene: the board as saved
        assert!(same_sound(&sound_board(&r), &r.board));
        let mut with = r.clone();
        let mut s = r.board.clone();
        s.blocks[0].on = !s.blocks[0].on;
        with.scenes[0] = Some(s.clone());
        let b = sound_board(&with);
        assert_eq!(b.blocks[0].on, s.blocks[0].on);
        assert_eq!(b.blocks.len(), r.board.blocks.len());
        let st = state_for(&with);
        assert_eq!(st.rig.as_deref(), Some("Lead"));
        assert_eq!((st.scene, st.based_on.as_str()), (0, "Reptilia · Lead"));
        assert!(st.scenes[0].is_some() && st.scenes[2].is_some());
    }

    #[test]
    fn room_state_round_trips() {
        let mut cfg = Config::default();
        assert_eq!(RoomState::load(&cfg), RoomState::default());
        let (_, b) = board::presets().swap_remove(0);
        let mut s = RoomState {
            rig: Some("Glass clean".into()),
            based_on: "Clean".into(),
            scene: 2,
            ..Default::default()
        };
        s.scenes[1] = Some(b);
        s.save(&mut cfg);
        let back = RoomState::load(&cfg);
        assert_eq!(back.rig, s.rig);
        assert_eq!(back.scene, 2);
        assert!(back.scenes[1].is_some() && back.scenes[0].is_none());
    }

    #[test]
    fn shuffle_makes_a_playable_rig() {
        let mut d = Dice::new(42);
        for _ in 0..50 {
            let b = shuffle(&mut d);
            assert!(b.first(Kind::Amp).is_some() && b.first(Kind::Cab).is_some());
            assert!(b.blocks.len() <= board::MAX_BLOCKS);
            for x in &b.blocks {
                for (k, kd) in x.def().knobs.iter().enumerate() {
                    assert_eq!(kd.clamp(x.v[k]), x.v[k]);
                }
            }
            assert!(!shuffle_name(&b).is_empty());
        }
    }
}
