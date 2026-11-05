//! The room driven through real egui frames (P38.1): drags on the chain, the catalogue's hover keys.

use super::*;
use crate::player::board::chip_keys;
use egui::{Event, PointerButton, Pos2, RawInput, pos2, vec2};
use qrs_audio::board::{Block, Board};

/// A room in its own data folder, the rig `ids`, and a context to draw it in.
pub(super) struct Rig {
    pub ctx: egui::Context,
    pub st: AppState,
    pub room: RoomView,
    dir: std::path::PathBuf,
    time: f64,
}

impl Rig {
    pub fn new(name: &str, ids: &[&str]) -> Rig {
        let ctx = egui::Context::default();
        qrs_ui::text::install(&ctx);
        let dir = std::env::temp_dir().join(format!("qrs-room-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut st = AppState::for_test(&ctx, dir.clone(), Default::default());
        st.config.set("room_seen", serde_json::json!(true));
        let mut room = RoomView::open(&mut st);
        room.rack.board_edit(&mut st, |b| {
            *b = Board::new(ids.iter().map(|id| Block::of(id).unwrap()).collect());
        });
        let mut r = Rig {
            ctx,
            st,
            room,
            dir,
            time: 0.0,
        };
        r.frame(Vec::new());
        r.frame(Vec::new());
        r
    }

    pub fn frame(&mut self, events: Vec<Event>) {
        self.time += 1.0 / 60.0;
        let input = RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1600.0, 1000.0))),
            time: Some(self.time),
            events,
            ..Default::default()
        };
        self.ctx.begin_pass(input);
        let (room, st) = (&mut self.room, &mut self.st);
        egui::Area::new(egui::Id::new("room-test"))
            .fixed_pos(Pos2::ZERO)
            .show(&self.ctx, |ui| {
                let area = Rect::from_min_size(Pos2::ZERO, vec2(1600.0, 1000.0));
                ui.set_min_size(area.size());
                room.ui(ui, st, area);
            });
        self.ctx.end_pass().textures_delta.clear();
    }

    fn button(pos: Pos2, pressed: bool) -> Event {
        Event::PointerButton {
            pos,
            button: PointerButton::Primary,
            pressed,
            modifiers: Default::default(),
        }
    }

    /// Move there, press, let go.
    pub fn click(&mut self, at: Pos2) {
        self.frame(vec![Event::PointerMoved(at)]);
        self.frame(vec![Event::PointerMoved(at)]);
        self.frame(vec![Self::button(at, true)]);
        self.frame(vec![Self::button(at, false)]);
        self.frame(Vec::new());
    }

    /// Press at `a`, move to `b` in steps, let go there.
    pub fn drag(&mut self, a: Pos2, b: Pos2) {
        self.frame(vec![Event::PointerMoved(a)]);
        self.frame(vec![Event::PointerMoved(a)]);
        self.frame(vec![Self::button(a, true)]);
        for k in 1..=15 {
            let q = a + (b - a) * (k as f32 / 15.0);
            self.frame(vec![Event::PointerMoved(q)]);
        }
        self.frame(vec![Self::button(b, false)]);
        self.frame(Vec::new());
    }

    pub fn order(&mut self) -> Vec<u32> {
        self.room
            .rack
            .board_now(&self.st)
            .blocks
            .iter()
            .map(|b| b.uid)
            .collect()
    }

    pub fn chip(&self, uid: u32) -> Rect {
        self.room
            .rack
            .board
            .chips
            .iter()
            .find(|c| c.0 == uid)
            .expect("drawn")
            .1
    }
}

impl Drop for Rig {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

#[test]
fn dragging_a_unit_in_the_rooms_chain_reorders_it() {
    let mut r = Rig::new("drag", &["green", "amp_crunch", "cab_4x12"]);
    let o = r.order();
    let (a, b) = (r.chip(o[0]), r.chip(o[1]));
    r.drag(a.center(), pos2(b.max.x + 8.0, a.center().y));
    assert_eq!(r.order(), vec![o[1], o[0], o[2]], "the pedal went after the amp");
    // and back to the front, from the end
    let o = r.order();
    let (a, c) = (r.chip(o[2]), r.chip(o[0]));
    r.drag(a.center(), pos2(c.min.x - 8.0, a.center().y));
    assert_eq!(r.order(), vec![o[2], o[0], o[1]], "the cab went to the front");
}

#[test]
fn a_drag_started_near_the_units_keys_still_moves_it() {
    // (the press lands on the unit's lower half, where the "i" dot shows on hover)
    let mut r = Rig::new("drag-low", &["green", "amp_crunch", "cab_4x12"]);
    let o = r.order();
    let (a, b) = (r.chip(o[0]), r.chip(o[1]));
    let from = pos2(a.center().x, a.max.y - a.height() * 0.3);
    r.drag(from, pos2(b.max.x + 8.0, from.y));
    assert_eq!(r.order(), vec![o[1], o[0], o[2]]);
}

#[test]
fn a_click_on_the_rooms_remove_key_takes_the_unit_off() {
    let mut r = Rig::new("x", &["green", "amp_crunch", "cab_4x12"]);
    let o = r.order();
    let x = chip_keys(r.chip(o[1])).0.center();
    r.click(x);
    assert_eq!(r.order(), vec![o[0], o[2]]);
}

/// The catalogue open on the rig, and its second card (not the selected one): its entry and rect.
fn catalogue(name: &str) -> (Rig, stage::Entry, Rect) {
    let mut r = Rig::new(name, &["green", "amp_crunch", "cab_4x12"]);
    r.room.cat = Some(catalogue::Cat::new(false));
    r.frame(Vec::new());
    r.frame(Vec::new());
    let c = r.room.cat.as_ref().expect("open");
    assert_eq!(c.sel, 0);
    let (e, rect) = c.cards[1];
    (r, e, rect)
}

#[test]
fn add_on_a_card_not_selected_adds_it_on_the_first_click() {
    let (mut r, e, card) = catalogue("cat-add");
    let n = r.order().len();
    // (the Add key: the first of the card's keys, at its bottom left)
    r.click(pos2(card.min.x + px(12.0) + px(10.0), card.max.y - px(26.0)));
    let b = r.room.rack.board_now(&r.st);
    assert_eq!(b.blocks.len(), n + 1, "added");
    let blk = e.block();
    assert!(b.blocks.iter().any(|x| x.model == blk.model));
}

#[test]
fn the_i_on_a_card_not_selected_opens_its_info_on_the_first_click() {
    let (mut r, e, card) = catalogue("cat-i");
    r.click(pos2(card.max.x - px(22.0), card.min.y + px(22.0)));
    assert_eq!(r.room.info.map(|x| x.key()), Some(e.key()), "its info card");
}

#[test]
fn no_text_prints_the_backtick() {
    // (P41.3, D172: the compare key's glyph read as a stray mark, "THE ` KEY FLIPS BACK"; it came back once, so no
    // string in the room's or the tile's code may hold it; the key is named in words, `AB_KEY`)
    for (name, src) in [
        ("mod.rs", include_str!("mod.rs")),
        ("rig.rs", include_str!("rig.rs")),
        ("strip.rs", include_str!("strip.rs")),
        ("sounds.rs", include_str!("sounds.rs")),
        ("catalogue.rs", include_str!("catalogue.rs")),
        ("save.rs", include_str!("save.rs")),
        ("plain.rs", include_str!("plain.rs")),
        ("board.rs", include_str!("../player/board.rs")),
    ] {
        for (n, line) in src.lines().enumerate() {
            let code = line.trim_start();
            if code.starts_with("//") {
                continue;
            }
            // (the parts inside string quotes)
            for (k, part) in code.split('"').enumerate() {
                assert!(
                    k % 2 == 0 || !part.contains('`'),
                    "{name}:{}: a backtick in the text: {line}",
                    n + 1
                );
            }
        }
    }
}

#[test]
fn the_catalogue_finds_players_and_songs_and_puts_the_family_first() {
    let find = |q: &str| {
        let mut c = catalogue::Cat::new(false);
        c.query = q.into();
        c.mode = rig::ShelfMode::Models;
        catalogue::entries(&c)
    };
    let big_muff = board::catalog::find("big_pi").map(stage::Entry::Model);
    for q in ["jack white", "Gilmour", "seven nation army"] {
        let all: Vec<stage::Entry> = find(q).into_iter().flat_map(|g| g.1).collect();
        assert!(all.contains(&big_muff.unwrap()), "{q}: {all:?}");
    }
    // "fuzz": the Fuzz group first, before a drive whose words say fuzz
    let g = find("fuzz");
    assert_eq!(
        g.first().map(|x| catalogue::GROUPS[x.0]),
        Some("Fuzz"),
        "{:?}",
        g.iter().map(|x| x.0).collect::<Vec<_>>()
    );
    assert_eq!(catalogue::wanted_group("delay pedal"), Some(10));
    assert_eq!(catalogue::wanted_group("jack white"), None);
}

#[test]
fn the_first_visit_puts_the_starters_first() {
    let mut r = Rig::new("starters", &["green", "amp_crunch", "cab_4x12"]);
    r.room.sounds.starters_first = true;
    r.room.go(&mut r.st, Tab::Sounds);
    r.frame(Vec::new());
    r.frame(Vec::new());
    let saved = r.room.saved.clone();
    r.room.sounds.moved();
    r.room.sounds.build(&saved, &r.st);
    assert!(
        matches!(r.room.sounds.first_item(), Some(sounds::Item::Start(_))),
        "the starting rigs come first"
    );
}
