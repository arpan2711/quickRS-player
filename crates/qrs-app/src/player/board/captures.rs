//! NAM captures and IR cabs on the board (P31.4, D80): the amp switch (MODELLED / NAM: one amp plays, the other is
//! off, the cabs follow), the key that chooses a block's capture, and the captures dialog (your files, add one from
//! disk, pick one on TONE3000). Shared by the player's board view and the amp room (both are a `Rack`).

use std::time::Duration;

use eframe::egui;
use egui::{Align2, Color32, CursorIcon, Rect, Sense, Ui, pos2, vec2};
use qrs_audio::board::{Block, Board, Kind, MODELS, catalog};
use qrs_ui::paint;
use qrs_ui::text::{self, Style};
use qrs_ui::theme::t;
use qrs_ui::units::px;
use qrs_ui::widgets::{self, ButtonStyle, interact};

use super::Rack;
use crate::captures::{self, Kind as CKind, tone3000};
use crate::state::AppState;

/// The captures dialog: which files it shows, the block it chooses for, a TONE3000 pick under way.
pub struct Dialog {
    pub kind: CKind,
    pub for_uid: Option<u32>,
    /// The download started here (`crate::downloads`; it carries on if the dialog closes).
    pick: Option<u64>,
    note: Option<String>,
    /// A remove asked for (a second click does it).
    removing: Option<u32>,
    /// The publishable key being typed (no key yet).
    key: String,
}

impl Dialog {
    pub fn new(kind: CKind, for_uid: Option<u32>) -> Dialog {
        Dialog {
            kind,
            for_uid,
            pick: None,
            note: None,
            removing: None,
            key: String::new(),
        }
    }
}

fn is_file(b: &Block, id: &str) -> bool {
    b.def().id == id
}

/// The kind of capture a file block plays.
pub(crate) fn kind_of(b: &Block) -> CKind {
    match b.def().id {
        "cab_ir" => CKind::Ir,
        "pedal_nam" => CKind::Pedal,
        _ => CKind::Nam,
    }
}

/// The model of ours a capture stands in for (its library skin).
pub(crate) fn skin_of(asset: u32) -> Option<String> {
    captures::with(|l| l.get(asset).and_then(|i| i.skin.clone()))
}

/// Whether a capture has its cab in it: the library's word (TONE3000's gear tag) first, then the file's.
pub(crate) fn has_cab(asset: u32) -> Option<bool> {
    captures::with(|l| l.get(asset).and_then(|i| i.cab))
        .or_else(|| qrs_audio::board::nam::capture_info(asset).and_then(|i| i.includes_cab))
}

/// Block `uid` is on: the other amps go off (one amp plays), and the cabs follow it. A NAM capture with its cab in
/// it plays with no cab; one without plays into an IR cab if the board has one, else into our cab. One of our amps
/// plays into our cab (an IR is off). A cab switched on is the only cab on.
pub fn engage(b: &mut Board, uid: u32, cab_of: impl Fn(u32) -> Option<bool>) {
    engage_with(b, uid, cab_of, skin_of)
}

/// `engage`, with where the skins come from (tests). A NAM pedal swaps only with its pair: the drive of ours it
/// stands in for (its skin). The user's other drives keep the on/off they were given (B10).
pub fn engage_with(
    b: &mut Board,
    uid: u32,
    cab_of: impl Fn(u32) -> Option<bool>,
    skin: impl Fn(u32) -> Option<String>,
) {
    let Some(i) = b.index_of(uid) else { return };
    let blk = b.blocks[i];
    if !blk.on {
        return;
    }
    match blk.def().kind {
        Kind::Amp => {
            for (j, o) in b.blocks.iter_mut().enumerate() {
                if j != i && o.def().kind == Kind::Amp {
                    o.on = false;
                }
            }
            let nam = is_file(&blk, "amp_nam");
            let ir = b.blocks.iter().position(|c| is_file(c, "cab_ir"));
            let ours: Vec<usize> = (0..b.blocks.len())
                .filter(|&j| b.blocks[j].def().kind == Kind::Cab && !is_file(&b.blocks[j], "cab_ir"))
                .collect();
            let set = |b: &mut Board, ir_on: bool, ours_on: bool| {
                if let Some(k) = ir {
                    b.blocks[k].on = ir_on;
                }
                // (our cabs: the first one on, if any should be and none is)
                if ours_on {
                    if !ours.iter().any(|&k| b.blocks[k].on)
                        && let Some(&k) = ours.first()
                    {
                        b.blocks[k].on = true;
                    }
                } else {
                    for &k in &ours {
                        b.blocks[k].on = false;
                    }
                }
            };
            match (nam, cab_of(blk.asset)) {
                (true, Some(true)) => set(b, false, false),
                (true, _) if ir.is_some() => set(b, true, false),
                (true, _) => set(b, false, true),
                (false, _) => set(b, false, true),
            }
            // a board with a NAM pedal: it plays with the NAM amp, our drives with ours (so the two rigs compare)
            let pairs: Vec<String> = b
                .blocks
                .iter()
                .filter(|x| is_file(x, "pedal_nam"))
                .filter_map(|x| skin(x.asset))
                .collect();
            // (P38.2: and the drive of ours right after a NAM pedal, the unit a song's NAM board kept beside it)
            let beside: Vec<u32> = b
                .blocks
                .windows(2)
                .filter(|w| {
                    is_file(&w[0], "pedal_nam") && matches!(w[1].def().kind, Kind::Drive | Kind::Fuzz)
                })
                .map(|w| w[1].uid)
                .collect();
            if b.blocks.iter().any(|x| is_file(x, "pedal_nam")) {
                for o in b.blocks.iter_mut() {
                    if is_file(o, "pedal_nam") {
                        o.on = nam;
                    } else if matches!(o.def().kind, Kind::Drive | Kind::Fuzz)
                        && (pairs.iter().any(|p| p == o.def().id) || beside.contains(&o.uid))
                    {
                        o.on = !nam;
                    }
                }
            }
        }
        Kind::Cab => {
            for (j, o) in b.blocks.iter_mut().enumerate() {
                if j != i && o.def().kind == Kind::Cab {
                    o.on = false;
                }
            }
        }
        _ => {}
    }
}

/// The capture `uid` plays into IR `ir`: the board's IR cab gets it, or one is put in after the capture.
pub(crate) fn with_ir(b: &mut Board, uid: u32, ir: u32) {
    if let Some(k) = b.blocks.iter().position(|c| is_file(c, "cab_ir")) {
        b.blocks[k].set_asset(ir);
        return;
    }
    let Some(i) = b.index_of(uid) else { return };
    if b.blocks.len() >= qrs_audio::board::MAX_BLOCKS {
        return;
    }
    if let Some(mut cab) = Block::of("cab_ir") {
        cab.set_asset(ir);
        cab.on = false;
        b.blocks.insert(i + 1, cab);
    }
}

impl Rack {
    /// The amp that plays: ours (`nam` false) or the NAM capture. A missing one is added (our first amp model, or a
    /// NAM capture after the selected amp, its capture the newest in your library; none yet opens the dialog).
    pub(crate) fn use_amp(&mut self, st: &mut AppState, nam: bool) {
        let b = self.board_now(st);
        let want = |x: &Block| x.def().kind == Kind::Amp && is_file(x, "amp_nam") == nam;
        if let Some(i) = b.blocks.iter().position(want) {
            let uid = b.blocks[i].uid;
            self.board_edit(st, |b| {
                b.blocks[i].on = true;
                engage(b, uid, has_cab);
            });
            self.board_commit(st);
            self.board.sel = Some(uid);
            if nam && b.blocks[i].asset == 0 {
                self.board.captures = Some(Dialog::new(CKind::Nam, Some(uid)));
            }
            return;
        }
        if b.blocks.len() >= qrs_audio::board::MAX_BLOCKS {
            self.say("The board is full", "  \u{b7}  12 blocks at most");
            return;
        }
        let at = b
            .blocks
            .iter()
            .position(|x| x.def().kind == Kind::Amp)
            .map(|i| i + 1)
            .unwrap_or(b.blocks.len());
        let model = if nam {
            catalog::find("amp_nam")
        } else {
            catalog::of_kind(Kind::Amp).next().map(|(i, _)| i)
        };
        let Some(model) = model else { return };
        let mut blk = Block::new(model);
        // (on a song's board: the capture, its IR and a pedal set up for the song first)
        let mut ir = None;
        let mut pedal = None;
        if nam {
            captures::sync();
            let title = self.song.title.clone();
            if let Some(it) = captures::with(|l| l.for_song(CKind::Nam, &title, true).cloned()) {
                blk.set_asset(it.id);
                ir = it.ir;
            }
            pedal = captures::with(|l| l.for_song(CKind::Pedal, &title, false).map(|i| i.id));
        }
        let uid = blk.uid;
        self.board_edit(st, |b| {
            b.blocks.insert(at.min(b.blocks.len()), blk);
            if let Some(i) = ir {
                with_ir(b, uid, i);
            }
            if let Some(p) = pedal
                && !b.blocks.iter().any(|x| is_file(x, "pedal_nam"))
                && b.blocks.len() < qrs_audio::board::MAX_BLOCKS
                && let Some(mut pb) = Block::of("pedal_nam")
            {
                // (in front of the amps)
                pb.set_asset(p);
                let first_amp = b
                    .blocks
                    .iter()
                    .position(|x| x.def().kind == Kind::Amp)
                    .unwrap_or(0);
                b.blocks.insert(first_amp, pb);
            }
            engage(b, uid, has_cab);
        });
        self.board_commit(st);
        self.board.sel = Some(uid);
        if nam && blk.asset == 0 {
            self.board.captures = Some(Dialog::new(CKind::Nam, Some(uid)));
        }
        self.say(format!("Added {}", MODELS[model].shown_name()), "");
    }

    /// Put a capture (or IR) on a block, and let it take over.
    pub(crate) fn set_asset(&mut self, st: &mut AppState, uid: u32, asset: u32) {
        let ir = captures::with(|l| l.get(asset).and_then(|i| i.ir));
        self.board_edit(st, |b| {
            if let Some(i) = b.index_of(uid) {
                b.blocks[i].set_asset(asset);
                b.blocks[i].on = true;
                if let Some(ir) = ir {
                    with_ir(b, uid, ir);
                }
                engage(b, uid, has_cab);
            }
        });
        self.board_commit(st);
        if let Some(n) = captures::name_of(asset) {
            self.say(format!("Playing {n}"), "");
        }
    }

    /// MODELLED / NAM: which amp plays (shown for an amp when captures are on). Returns whether it was drawn.
    pub(crate) fn amp_switch(&mut self, ui: &Ui, st: &mut AppState, r: Rect, blk: Block) -> bool {
        if !captures::ON || blk.def().kind != Kind::Amp {
            return false;
        }
        let b = self.board_now(st);
        let nam_on = b.blocks.iter().any(|x| x.on && is_file(x, "amp_nam"));
        let ours_on = b
            .blocks
            .iter()
            .any(|x| x.on && x.def().kind == Kind::Amp && !is_file(x, "amp_nam"));
        let sel = match (ours_on, nam_on) {
            (_, true) => Some(1),
            (true, false) => Some(0),
            _ => None,
        };
        let p = ui.painter().clone();
        let rs = super::pills(&p, r, &["MODELLED".to_string(), "NAM".to_string()], sel, 10.0);
        for (k, pr) in rs.iter().enumerate() {
            let resp = interact(ui, *pr, ("amp-switch", k), Sense::click());
            if resp.hovered() {
                ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
                self.tip = Some((
                    if k == 0 {
                        "Play our modelled amp (the NAM capture goes off)".into()
                    } else {
                        "Play a NAM capture (our amp goes off)".into()
                    },
                    String::new(),
                    pos2(pr.center().x, pr.min.y - px(6.0)),
                    Align2::CENTER_BOTTOM,
                ));
            }
            if resp.clicked() && sel != Some(k) {
                self.use_amp(st, k == 1);
            }
        }
        true
    }

    /// The name a file block shows: its capture's (or "No capture yet").
    pub(crate) fn file_name(blk: &Block) -> Option<String> {
        if !blk.def().file {
            return None;
        }
        Some(match captures::name_of(blk.asset) {
            Some(n) => n,
            None if blk.asset == 0 => "No file chosen yet".into(),
            None => "Its file isn't in your library".into(),
        })
    }

    /// Open the dialog for a file block.
    pub(crate) fn choose_file(&mut self, blk: Block) {
        let kind = if is_file(&blk, "cab_ir") {
            CKind::Ir
        } else if is_file(&blk, "pedal_nam") {
            CKind::Pedal
        } else {
            CKind::Nam
        };
        self.board.captures = Some(Dialog::new(kind, Some(blk.uid)));
    }

    /// The captures dialog, over `area` (a click outside closes it).
    pub(crate) fn captures_ui(&mut self, ui: &Ui, st: &mut AppState, area: Rect) {
        let Some(mut d) = self.board.captures.take() else {
            return;
        };
        let keep = self.dialog(ui, st, area, &mut d);
        if keep {
            self.board.captures = Some(d);
        }
    }

    fn dialog(&mut self, ui: &Ui, st: &mut AppState, area: Rect, d: &mut Dialog) -> bool {
        let p = ui.painter().clone();
        let th = t();
        let ctx = ui.ctx().clone();
        paint::dim(&p, area, Some(215));
        let w = px(640.0).min(area.width() - px(40.0));
        let h = px(560.0).min(area.height() - px(40.0));
        let panel = Rect::from_center_size(area.center(), vec2(w, h));
        let back = interact(ui, area, "captures-back", Sense::click());
        if back.clicked() && back.interact_pointer_pos().is_some_and(|q| !panel.contains(q)) {
            return false;
        }
        // (the panel takes the clicks that miss its keys)
        let _ = interact(ui, panel, "captures-panel", Sense::click());
        // (solid under the glass: the board's keys mustn't show through)
        p.rect_filled(panel, px(24.0), th.base);
        paint::floating(&p, panel, Some(24.0));
        let inner = panel.shrink(px(22.0));
        // header: the title, the kind, close
        text::caps(
            &p,
            "YOUR CAPTURES",
            pos2(inner.min.x, inner.min.y + px(8.0)),
            10.5,
            th.caption,
            Align2::LEFT_CENTER,
            None,
        );
        text::draw(
            &p,
            match d.kind {
                CKind::Nam => "NAM amps",
                CKind::Pedal => "NAM pedals",
                CKind::Ir => "IR cabs",
            },
            pos2(inner.min.x, inner.min.y + px(32.0)),
            Align2::LEFT_CENTER,
            Style::new(20.0, 800, th.text),
            None,
        );
        let xr = Rect::from_min_size(
            pos2(inner.max.x - px(34.0), inner.min.y),
            vec2(px(34.0), px(34.0)),
        );
        if widgets::button(
            ui,
            xr,
            "captures-close",
            "",
            "",
            ButtonStyle {
                icon: Some(qrs_ui::icons::Icon::X),
                ..Default::default()
            },
        )
        .clicked()
        {
            return false;
        }
        let tr = Rect::from_min_size(
            pos2(xr.min.x - px(12.0) - px(250.0), inner.min.y + px(2.0)),
            vec2(px(250.0), px(30.0)),
        );
        const TABS: [CKind; 3] = [CKind::Nam, CKind::Pedal, CKind::Ir];
        let tabs = super::pills(
            &p,
            tr,
            &["AMPS".to_string(), "PEDALS".to_string(), "IR CABS".to_string()],
            TABS.iter().position(|k| *k == d.kind),
            10.0,
        );
        for (k, r) in tabs.iter().enumerate() {
            if interact(ui, *r, ("captures-tab", k), Sense::click()).clicked() {
                let want = TABS[k];
                if want != d.kind {
                    d.kind = want;
                    // (the block it chose for is the other kind's)
                    d.for_uid = None;
                }
            }
        }
        // the block it's for, if any
        let b = self.board_now(st);
        let for_blk = d.for_uid.and_then(|u| b.index_of(u)).map(|i| b.blocks[i]);
        let mut y = inner.min.y + px(60.0);
        let sub = match for_blk {
            Some(blk) => format!(
                "Click one to play it on the board's {}.",
                if is_file(&blk, "cab_ir") {
                    "IR cab"
                } else if is_file(&blk, "pedal_nam") {
                    "NAM pedal"
                } else {
                    "NAM capture"
                }
            ),
            None => "Your files, kept in quickRS's data folder.".into(),
        };
        text::draw(
            &p,
            &sub,
            pos2(inner.min.x, y),
            Align2::LEFT_CENTER,
            Style::new(12.0, 600, th.caption),
            Some(inner.width()),
        );
        y += px(18.0);
        // the list
        let foot_h = px(150.0);
        let list = Rect::from_min_max(pos2(inner.min.x, y), pos2(inner.max.x, inner.max.y - foot_h));
        paint::inset(&p, list, Some(14.0), 2.0, paint::Fill::Base);
        let items: Vec<captures::Item> = captures::with(|l| {
            let mut v: Vec<captures::Item> = l.of(d.kind).cloned().collect();
            v.sort_by(|a, c| c.added.total_cmp(&a.added));
            v
        });
        let rh = px(52.0);
        if items.is_empty() {
            text::draw(
                &p,
                match d.kind {
                    CKind::Nam => "No NAM captures yet: add a .nam file, or pick one on TONE3000.",
                    CKind::Pedal => "No NAM pedals yet: add a .nam file of a pedal, or pick one on TONE3000.",
                    CKind::Ir => "No IR cabs yet: add a .wav impulse response, or pick one on TONE3000.",
                },
                list.center(),
                Align2::CENTER_CENTER,
                Style::new(13.0, 600, th.caption),
                Some(list.width() - px(40.0)),
            );
        }
        let rows = ((list.height() - px(8.0)) / rh).floor().max(1.0) as usize;
        for (n, it) in items.iter().take(rows).enumerate() {
            let rr = Rect::from_min_size(
                pos2(list.min.x + px(6.0), list.min.y + px(4.0) + rh * n as f32),
                vec2(list.width() - px(12.0), rh - px(4.0)),
            );
            let on = for_blk.is_some_and(|f| f.asset == it.id);
            let resp = interact(ui, rr, ("captures-row", it.id), Sense::click());
            if on {
                paint::display(&p, rr, Some(10.0), 3.0);
            } else if resp.hovered() {
                p.rect_filled(rr, px(10.0), Color32::from_white_alpha(10));
            }
            if resp.hovered() && for_blk.is_some() {
                ctx.set_cursor_icon(CursorIcon::PointingHand);
            }
            let del = Rect::from_min_size(
                pos2(rr.max.x - px(86.0), rr.center().y - px(14.0)),
                vec2(px(80.0), px(28.0)),
            );
            let name_w = del.min.x - rr.min.x - px(24.0);
            text::draw(
                &p,
                &it.name,
                pos2(rr.min.x + px(12.0), rr.min.y + px(15.0)),
                Align2::LEFT_CENTER,
                Style::new(14.0, 800, if on { th.accent } else { th.text }),
                Some(name_w),
            );
            text::draw(
                &p,
                &it.detail(),
                pos2(rr.min.x + px(12.0), rr.min.y + px(34.0)),
                Align2::LEFT_CENTER,
                Style::new(11.0, 600, th.caption),
                Some(name_w),
            );
            let sure = d.removing == Some(it.id);
            if widgets::button(
                ui,
                del,
                ("captures-del", it.id),
                if sure { "Delete it" } else { "Remove" },
                "",
                ButtonStyle {
                    danger: sure,
                    sunk: !sure,
                    size: 11.5,
                    ..Default::default()
                },
            )
            .clicked()
            {
                if sure {
                    captures::with(|l| l.remove(it.id));
                    d.removing = None;
                    d.note = Some(format!("Removed {}", it.name));
                } else {
                    d.removing = Some(it.id);
                }
            } else if resp.clicked()
                && let Some(f) = for_blk
            {
                self.set_asset(st, f.uid, it.id);
                d.removing = None;
            }
        }
        if items.len() > rows {
            text::draw(
                &p,
                &format!("+{} more", items.len() - rows),
                pos2(list.max.x - px(12.0), list.max.y - px(10.0)),
                Align2::RIGHT_CENTER,
                Style::new(11.0, 600, th.caption),
                None,
            );
        }
        // the foot: add a file, TONE3000, what's happening
        let fy = list.max.y + px(16.0);
        let add = Rect::from_min_size(pos2(inner.min.x, fy), vec2(px(170.0), px(36.0)));
        if widgets::button(
            ui,
            add,
            "captures-add",
            if d.kind == CKind::Ir {
                "Add a .wav IR\u{2026}"
            } else {
                "Add a .nam file\u{2026}"
            },
            "",
            ButtonStyle::default(),
        )
        .clicked()
        {
            let (what, ext) = if d.kind == CKind::Ir {
                ("Impulse response", "wav")
            } else {
                ("NAM capture", "nam")
            };
            let picked = rfd::FileDialog::new()
                .set_title(match d.kind {
                    CKind::Nam => "Add a NAM capture",
                    CKind::Pedal => "Add a NAM pedal",
                    CKind::Ir => "Add an IR cab",
                })
                .add_filter(what, &[ext])
                .pick_file();
            if let Some(path) = picked {
                match captures::with(|l| l.add_path_as(&path, Some(d.kind))) {
                    Ok(it) => {
                        d.note = Some(format!("Added {}", it.name));
                        if let Some(f) = for_blk {
                            self.set_asset(st, f.uid, it.id);
                        }
                    }
                    Err(e) => d.note = Some(e),
                }
            }
        }
        let t3 = Rect::from_min_size(pos2(add.max.x + px(10.0), fy), vec2(px(220.0), px(36.0)));
        let key = tone3000::client_id(&st.config);
        let busy = d
            .pick
            .and_then(crate::downloads::view)
            .is_some_and(|v| v.phase == tone3000::Phase::InBrowser);
        if widgets::button(
            ui,
            t3,
            "captures-t3k",
            if busy {
                "Cancel"
            } else {
                "Pick on TONE3000\u{2026}"
            },
            "",
            if busy {
                ButtonStyle::default()
            } else {
                ButtonStyle::primary()
            },
        )
        .clicked()
        {
            if busy {
                if let Some(id) = d.pick {
                    crate::downloads::cancel(id);
                }
            } else if let Some(k) = key.clone() {
                d.pick = Some(crate::downloads::start(k, d.kind, None));
                d.note = None;
            } else {
                d.note = Some("TONE3000 needs your key first (below).".into());
            }
        }
        // the pick's progress (the card over the page shows it; here a line, and the block gets the capture)
        if let Some(id) = d.pick {
            match crate::downloads::view(id).map(|v| (v.short(), v.phase)) {
                Some((_, tone3000::Phase::Done(it))) => {
                    d.note = Some(format!("Added {} from TONE3000", it.name));
                    if let Some(f) = for_blk {
                        self.set_asset(st, f.uid, it.id);
                    }
                    d.pick = None;
                }
                Some((_, tone3000::Phase::Failed(_, e))) => {
                    d.note = Some(format!("Didn't download: {e}"));
                    d.pick = None;
                }
                Some((_, tone3000::Phase::Cancelled { .. })) | None => {
                    d.note = Some("Nothing picked.".into());
                    d.pick = None;
                }
                Some((_, tone3000::Phase::InBrowser)) => {
                    d.note = Some(
                        "Pick the tone on the TONE3000 page (sign in there the first time); quickRS downloads it here."
                            .into(),
                    )
                }
                Some((short, _)) => d.note = Some(format!("TONE3000: {short}")),
            }
            ctx.request_repaint_after(Duration::from_millis(200));
        }
        if let Some(n) = &d.note {
            text::draw(
                &p,
                n,
                pos2(t3.max.x + px(14.0), fy + px(18.0)),
                Align2::LEFT_CENTER,
                Style::new(12.0, 600, th.text_2),
                Some(inner.max.x - t3.max.x - px(14.0)),
            );
        }
        // no key yet: where to get it, and a field for it
        let ky = fy + px(52.0);
        if key.is_none() {
            text::draw(
                &p,
                "Your TONE3000 key: on tone3000.com, Settings > API Keys > create one, then paste its \
                 publishable key here.",
                pos2(inner.min.x, ky),
                Align2::LEFT_CENTER,
                Style::new(11.5, 600, th.caption),
                Some(inner.width()),
            );
            let fr = Rect::from_min_size(
                pos2(inner.min.x, ky + px(16.0)),
                vec2(inner.width() - px(110.0), px(30.0)),
            );
            // (a text field of egui's own, in an area over the dialog: the dialog draws with a shared `Ui`)
            egui::Area::new(egui::Id::new("captures-key-field"))
                .fixed_pos(fr.min)
                .order(egui::Order::Foreground)
                .show(&ctx, |ui| {
                    ui.add_sized(
                        fr.size(),
                        egui::TextEdit::singleline(&mut d.key)
                            .hint_text("t3k_pub_...")
                            .font(egui::FontId::proportional(13.0)),
                    );
                });
            let sr = Rect::from_min_size(pos2(fr.max.x + px(10.0), fr.min.y), vec2(px(100.0), px(30.0)));
            let k = d.key.trim().to_string();
            if widgets::button(
                ui,
                sr,
                "captures-key",
                "Save key",
                "",
                ButtonStyle {
                    enabled: k.starts_with("t3k_pub_"),
                    ..Default::default()
                },
            )
            .clicked()
            {
                st.config.set("tone3000_client_id", serde_json::Value::from(k));
                st.save_config();
                d.note = Some("Key saved: Pick on TONE3000 is ready.".into());
            }
        }
        // the free tier's credit (a click opens the site)
        let pw = Rect::from_min_size(
            pos2(inner.min.x, inner.max.y - px(18.0)),
            vec2(px(200.0), px(18.0)),
        );
        let pr = text::draw(
            &p,
            "Powered by TONE3000",
            pw.left_center(),
            Align2::LEFT_CENTER,
            Style::new(11.5, 800, th.text_2),
            None,
        );
        let presp = interact(ui, pr, "captures-t3k-site", Sense::click());
        if presp.hovered() {
            ctx.set_cursor_icon(CursorIcon::PointingHand);
        }
        if presp.clicked() {
            crate::update::open_url(tone3000::SITE);
        }
        text::draw(
            &p,
            "Each capture keeps its creator and licence. Captures run on Neural Amp Modeler (MIT).",
            pos2(pr.max.x + px(12.0), pr.center().y),
            Align2::LEFT_CENTER,
            Style::new(10.5, 600, th.caption),
            Some(inner.max.x - pr.max.x - px(12.0)),
        );
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn board(ids: &[(&str, bool)]) -> Board {
        Board::new(
            ids.iter()
                .map(|(id, on)| {
                    let mut b = Block::of(id).unwrap();
                    b.on = *on;
                    b
                })
                .collect(),
        )
    }

    pub(super) fn ons(b: &Board) -> Vec<bool> {
        b.blocks.iter().map(|x| x.on).collect()
    }

    #[test]
    fn one_amp_plays_and_the_cabs_follow() {
        let ours = catalog::of_kind(Kind::Amp).next().unwrap().1.id;
        let cab = catalog::of_kind(Kind::Cab).next().unwrap().1.id;
        // our amp and cab, a capture without its cab, an IR
        let mut b = board(&[(ours, true), (cab, true), ("amp_nam", true), ("cab_ir", false)]);
        let nam = b.blocks[2].uid;
        engage(&mut b, nam, |_| Some(false));
        assert_eq!(
            ons(&b),
            [false, false, true, true],
            "the capture plays into the IR"
        );
        // our amp back: our cab, not the IR
        b.blocks[0].on = true;
        let uid = b.blocks[0].uid;
        engage(&mut b, uid, |_| None);
        assert_eq!(ons(&b), [true, true, false, false]);
        // a capture with its cab: no cab at all
        let mut b = board(&[(ours, true), (cab, true), ("amp_nam", true)]);
        let nam = b.blocks[2].uid;
        engage(&mut b, nam, |_| Some(true));
        assert_eq!(ons(&b), [false, false, true]);
        // a capture without a cab and no IR: into our cab
        let mut b = board(&[(ours, true), (cab, false), ("amp_nam", true)]);
        let nam = b.blocks[2].uid;
        engage(&mut b, nam, |_| Some(false));
        assert_eq!(ons(&b), [false, true, true]);
    }
}

#[cfg(test)]
mod pedal_tests {
    use super::*;

    #[test]
    fn a_nam_pedal_goes_with_the_nam_amp() {
        let ours = catalog::of_kind(Kind::Amp).next().unwrap().1.id;
        let drive = catalog::of_kind(Kind::Drive).next().unwrap().1.id;
        let mut b = Board::new(
            [drive, "pedal_nam", ours, "amp_nam"]
                .iter()
                .map(|id| Block::of(id).unwrap())
                .collect(),
        );
        let nam = b.blocks[3].uid;
        let skin = |_| Some(drive.to_string());
        engage_with(&mut b, nam, |_| Some(true), skin);
        assert_eq!(
            b.blocks.iter().map(|x| x.on).collect::<Vec<_>>(),
            [false, true, false, true]
        );
        b.blocks[2].on = true;
        let uid = b.blocks[2].uid;
        engage_with(&mut b, uid, |_| None, skin);
        assert_eq!(
            b.blocks.iter().map(|x| x.on).collect::<Vec<_>>(),
            [true, false, true, false]
        );
    }

    #[test]
    fn engage_leaves_the_users_other_drives_alone() {
        // a Rodent the user turned off, a Mean Green the NAM pedal stands in for (B10)
        let ours = catalog::of_kind(Kind::Amp).next().unwrap().1.id;
        let mut b = Board::new(
            ["rodent", "green", "pedal_nam", ours, "amp_nam"]
                .iter()
                .map(|id| Block::of(id).unwrap())
                .collect(),
        );
        b.blocks[0].on = false;
        let skin = |_| Some("green".to_string());
        let nam = b.blocks[4].uid;
        engage_with(&mut b, nam, |_| Some(true), skin);
        assert_eq!(super::tests::ons(&b), [false, false, true, false, true]);
        let uid = b.blocks[3].uid;
        b.blocks[3].on = true;
        engage_with(&mut b, uid, |_| None, skin);
        // (the Rodent stays off: only the pair swapped)
        assert_eq!(super::tests::ons(&b), [false, true, false, true, false]);
        // a NAM pedal with no skin swaps with nothing of ours
        let mut c = Board::new(
            ["rodent", "pedal_nam", "amp_nam"]
                .iter()
                .map(|id| Block::of(id).unwrap())
                .collect(),
        );
        let nam = c.blocks[2].uid;
        engage_with(&mut c, nam, |_| Some(true), |_| None);
        assert_eq!(super::tests::ons(&c), [true, true, true]);
    }
}
