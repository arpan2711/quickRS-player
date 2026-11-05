//! The REDUCE DELAY card, drawn in two places (P41.4, D174; the card is P35.4's, its highlight the owner's mockup C
//! of 2026-10-29): Settings > Timing, and first on the song's Controls > Practise tab, so the fast output catches the
//! eye and can be switched mid-song. One drawing and one action for both:
//!
//!     REDUCE DELAY                              [in 10.6 ms + out 10.6 ms  ·  WASAPI exclusive]
//!     ( FAST  Low-latency output         (o ) )    Buffer
//!     (       skips the mixer; other apps go quiet)  smaller is quicker; raise it if you hear crackles
//!                                                   [Auto | 64 | 128 | 256 | 512]
//!
//! The Low-latency output switch sits on a glowing, tinted plate with a FAST badge (emerald in the dark themes,
//! orange in Daylight). Too narrow for two columns (the Controls panel), the buffer goes under the plate and the
//! latency chip under the heading if it doesn't fit beside it; nothing is cut short. Switching reopens the output
//! (WASAPI exclusive or shared; ASIO is exclusive anyway, so the chip reads ASIO and the switch only matters for a
//! WASAPI output); the engine keeps the song, its position and whether it's playing across the reopen
//! (`engine_null::reopening_keeps_the_song_and_position`), so a song plays on after a moment's gap.

use eframe::egui;
use egui::{Align2, Color32, Rect, Ui, pos2, vec2};
use qrs_audio::backend::StreamInfo;
use qrs_audio::devices::AudioPrefs;
use qrs_ui::paint::{self, Fill};
use qrs_ui::text::{self, Style};
use qrs_ui::theme::t;
use qrs_ui::units::px;
use qrs_ui::widgets::{self, Segmented, interact, switch_size};

use super::{centre_x, centre_y, line_h, snap};
use crate::state::{AppState, AudioMode};

/// The buffer's choices (frames; None: the driver's own).
pub const BUFFERS: [(&str, Option<u32>); 5] = [
    ("Auto", None),
    ("64", Some(64)),
    ("128", Some(128)),
    ("256", Some(256)),
    ("512", Some(512)),
];

/// The switch's words (the same in both places).
pub const LABEL: &str = "Low-latency output";
pub const HINT: &str = "skips the mixer; other apps go quiet";
pub const BADGE: &str = "FAST";
pub const HEADING: &str = "REDUCE DELAY";
const BUFFER_HINT: &str = "smaller is quicker; raise it if you hear crackles";

/// What the card shows.
#[derive(Clone, Debug, PartialEq)]
pub struct Shown {
    pub on: bool,
    pub buffer: Option<u32>,
    pub latency: String,
}

impl Shown {
    pub fn of(st: &AppState) -> Shown {
        let prefs = st.audio.as_ref().map(|a| a.prefs()).unwrap_or_default();
        Shown {
            on: prefs.exclusive,
            buffer: prefs.buffer,
            latency: latency_text(st),
        }
    }
}

/// What a click on the card asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Act {
    Toggle,
    Buffer(Option<u32>),
}

impl Act {
    /// As an action string: Settings' own ("exclusive", "buffer:128", "buffer:auto"); the player prefixes "fastout:".
    pub fn action(self) -> String {
        match self {
            Act::Toggle => "exclusive".into(),
            Act::Buffer(None) => "buffer:auto".into(),
            Act::Buffer(Some(n)) => format!("buffer:{n}"),
        }
    }

    pub fn parse(s: &str) -> Option<Act> {
        match s.split_once(':') {
            None if s == "exclusive" => Some(Act::Toggle),
            Some(("buffer", "auto")) => Some(Act::Buffer(None)),
            Some(("buffer", n)) => n.parse().ok().map(|n| Act::Buffer(Some(n))),
            _ => None,
        }
    }
}

/// The preferences after `act`, and what to say about it.
pub fn set(prefs: &mut AudioPrefs, act: Act) -> String {
    match act {
        Act::Toggle => {
            prefs.exclusive = !prefs.exclusive;
            if prefs.exclusive {
                "Low-latency output: other programs go quiet while quickRS plays".into()
            } else {
                "Shared output: other programs can play too".into()
            }
        }
        Act::Buffer(b) => {
            prefs.buffer = b;
            match b {
                None => "Buffer: Auto (the device's own)".into(),
                Some(n) => format!("Buffer: {n} frames"),
            }
        }
    }
}

/// Does `act` (from either place): the device manager re-plans and reopens the output, config.json "audio" keeps
/// it. Ok: what to say; Err: why nothing changed.
pub fn apply(st: &mut AppState, act: Act) -> Result<String, String> {
    if st.audio_mode == AudioMode::Silent {
        return Err("Silent mode: quickRS isn't using any sound device".into());
    }
    let Some(audio) = &st.audio else {
        return Err("The audio engine isn't running".into());
    };
    let mut prefs = audio.prefs();
    let say = set(&mut prefs, act);
    audio.set_prefs(prefs.clone());
    st.config
        .set("audio", serde_json::to_value(&prefs).unwrap_or_default());
    st.save_config();
    Ok(say)
}

/// The in / out latency the engine reports, for the chip ("in 10.6 ms + out 10.6 ms · ASIO").
pub fn latency_text(st: &AppState) -> String {
    let Some(audio) = &st.audio else {
        return "no audio engine".into();
    };
    let status = audio.engine.status();
    let ms = |i: &StreamInfo| (i.block as f64 / i.rate.max(1) as f64 + i.latency) * 1000.0;
    match (&status.input, &status.output) {
        _ if st.audio_mode == AudioMode::Silent => "silent".to_string(),
        (Some(i), Some(o)) if status.duplex => {
            format!("in {:.1} ms + out {:.1} ms  \u{b7}  {}", ms(i), ms(o), o.mode)
        }
        (Some(i), Some(o)) => format!(
            "in {:.1} ms + out {:.1} ms  \u{b7}  {}",
            ms(i),
            ms(o),
            if i.mode == o.mode {
                o.mode.clone()
            } else {
                format!("{} / {}", i.mode, o.mode)
            }
        ),
        (None, Some(o)) => format!("out {:.1} ms  \u{b7}  {}", ms(o), o.mode),
        (Some(i), None) => format!("in {:.1} ms  \u{b7}  {}", ms(i), i.mode),
        (None, None) => "no devices".into(),
    }
}

/// Where the card sits: Settings' raised card, or a well in the Controls panel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Frame {
    Settings,
    Panel,
}

/// The highlight's colour: emerald in the dark themes, orange in Daylight.
pub fn highlight() -> Color32 {
    if qrs_ui::theme::is_dark() {
        Color32::from_rgb(52, 199, 123)
    } else {
        Color32::from_rgb(232, 112, 30)
    }
}

/// Where everything goes (worked out once, for drawing and for the tests).
#[derive(Clone, Debug, PartialEq)]
pub struct Layout {
    pub card: Rect,
    pub heading: egui::Pos2,
    pub chip: Rect,
    pub plate: Rect,
    pub badge: Rect,
    /// The switch row on the plate (its label, its hint lines, the switch at its right).
    pub row: Rect,
    pub switch: Rect,
    pub hint_lines: Vec<String>,
    /// "Buffer" (its top left), its hint (top left, width), the choices.
    pub buffer_title: egui::Pos2,
    pub buffer_hint: (egui::Pos2, f32),
    pub seg: Rect,
    /// Buffer beside the plate (Settings) or under it (the panel).
    pub columns: bool,
}

/// The row's height on the plate (Settings' `switch_row` "lg" with a hint).
fn h_switch() -> f32 {
    let sh = switch_size("lg").y;
    px(sh + 8.0).max(px(40.0))
}

fn chip_style() -> Style {
    Style::new(12.0, 700, t().text_2)
}

fn badge_style() -> Style {
    Style::new(11.0, 800, Color32::from_rgb(6, 34, 20))
}

pub fn layout(ctx: &egui::Context, x0: f32, y0: f32, w: f32, frame: Frame, shown: &Shown) -> Layout {
    let (pad, gap) = match frame {
        Frame::Settings => (px(22.0), px(28.0)),
        Frame::Panel => (px(12.0), px(12.0)),
    };
    let hs = h_switch();
    let (x, wd) = (x0 + pad, w - 2.0 * pad);
    let colw2 = snap((wd - gap) / 2.0);
    // two columns while each has room for the plate's words (Settings at any usual size)
    let columns = frame == Frame::Settings && colw2 >= px(300.0);
    let heading = pos2(x, y0 + pad);
    let hst_caps = Style::caps(12.0, t().caption);
    // (as Settings' `heading`: 12 under the caps line, then 8)
    let mut yy = heading.y + text::line_height(ctx, hst_caps) + px(12.0) + px(8.0);
    let tw = text::width(ctx, &shown.latency, chip_style()) + px(20.0);
    let head_w = text::width(ctx, HEADING, hst_caps);
    let chip = if columns {
        let lw = tw.min(snap(w / 2.0));
        Rect::from_min_size(pos2(x0 + w - pad - lw, y0 + pad - px(6.0)), vec2(lw, px(28.0)))
    } else if x + head_w + px(16.0) + tw <= x + wd {
        Rect::from_min_size(pos2(x + wd - tw, y0 + pad - px(6.0)), vec2(tw, px(28.0)))
    } else {
        // its own line under the heading, as wide as it needs
        let r = Rect::from_min_size(pos2(x, yy - px(4.0)), vec2(tw.min(wd), px(28.0)));
        yy = r.max.y + px(10.0);
        r
    };
    let colw = if columns { colw2 } else { wd };
    // the plate: the badge, then the switch row (label, hint, the switch at the right)
    let lx = x + px(12.0);
    let lwd = colw - px(24.0);
    let bw = text::width(ctx, BADGE, badge_style()) + px(16.0);
    let sw = switch_size("lg");
    // the badge before the label, or over it when the label wouldn't fit beside it (a very narrow panel)
    let label_w = text::width(ctx, LABEL, Style::new(15.0, 700, Color32::WHITE));
    let above = label_w > lwd - bw - px(8.0) - px(sw.x) - px(12.0);
    let (rx, rw, top) = if above {
        (lx, lwd, px(20.0))
    } else {
        (lx + bw + px(8.0), lwd - bw - px(8.0), 0.0)
    };
    let hint_max = rw - px(sw.x) - px(20.0);
    let hst = Style::new(12.0, 500, Color32::WHITE);
    let hint_lines = if text::width(ctx, HINT, hst) <= hint_max {
        vec![HINT.to_string()]
    } else {
        text::wrap(ctx, HINT, hst, hint_max)
    };
    let extra = (hint_lines.len().max(1) - 1) as f32 * line_h(ctx, 12.0) + top;
    let plate = Rect::from_min_size(pos2(x, yy), vec2(colw, hs + px(14.0) + extra));
    let ly = yy + px(7.0);
    let row = Rect::from_min_size(pos2(rx, ly + top), vec2(rw, hs + extra - top));
    let swr = Rect::from_min_size(
        pos2(row.max.x - px(sw.x), centre_y(row) - snap(px(sw.y) / 2.0)),
        vec2(px(sw.x), px(sw.y)),
    );
    let badge = if above {
        Rect::from_min_size(pos2(lx, ly + px(2.0)), vec2(bw, px(16.0)))
    } else {
        Rect::from_min_size(pos2(lx, ly + hs / 2.0 - px(9.0) - px(8.0)), vec2(bw, px(16.0)))
    };
    // the buffer: beside the plate, or under it with the choices beside its name
    let seg_h = px(40.0);
    let (buffer_title, buffer_hint, seg, bottom) = if columns {
        let bx = x + colw + gap;
        let t_h = line_h(ctx, 15.0);
        let hint_y = yy + t_h + px(4.0);
        let seg = Rect::from_min_size(pos2(bx, hint_y + line_h(ctx, 12.0) + px(12.0)), vec2(colw, seg_h));
        let right_h = t_h + px(4.0) + line_h(ctx, 12.0) + px(12.0) + seg_h;
        let bottom = yy + (hs + px(14.0) + extra).max(right_h) + pad;
        (pos2(bx, yy), (pos2(bx, hint_y), colw), seg, bottom)
    } else {
        let by = plate.max.y + px(14.0);
        let tw_b = text::width(ctx, "Buffer", Style::new(15.0, 700, Color32::WHITE));
        let seg_h = px(34.0);
        let seg = Rect::from_min_size(pos2(x + tw_b + px(16.0), by), vec2(wd - tw_b - px(16.0), seg_h));
        let title = pos2(x, by + snap(seg_h / 2.0) - snap(line_h(ctx, 15.0) / 2.0));
        let hint_y = seg.max.y + px(6.0);
        let bottom = hint_y + line_h(ctx, 12.0) * text::wrap(ctx, BUFFER_HINT, hst, wd).len() as f32 + pad;
        (title, (pos2(x, hint_y), wd), seg, bottom)
    };
    let card = Rect::from_min_max(pos2(x0, y0), pos2(x0 + w, bottom.max(plate.max.y + pad)));
    Layout {
        card,
        heading,
        chip,
        plate,
        badge,
        row,
        switch: swr,
        hint_lines,
        buffer_title,
        buffer_hint,
        seg,
        columns,
    }
}

/// Draws the card with its top left at (`x`, `y`), `w` wide. `salt` keeps its ids apart from the other place's
/// (Settings can be open over the song's panel). Returns the card's rect and what was clicked.
pub fn draw(
    ui: &Ui,
    x: f32,
    y: f32,
    w: f32,
    frame: Frame,
    shown: &Shown,
    salt: &'static str,
) -> (Rect, Option<Act>) {
    let th = t();
    let p = ui.painter().clone();
    let ctx = ui.ctx().clone();
    let l = layout(&ctx, x, y, w, frame, shown);
    let mut act = None;
    match frame {
        Frame::Settings => paint::card(&p, l.card, 24.0, false),
        Frame::Panel => paint::inset(&p, l.card, Some(18.0), 5.0, Fill::Base),
    }
    text::caps(&p, HEADING, l.heading, 12.0, th.caption, Align2::LEFT_TOP, None);
    // the latency chip
    paint::display(&p, l.chip, Some(9.0), 3.0);
    text::draw(
        &p,
        &shown.latency,
        pos2(centre_x(l.chip), centre_y(l.chip)),
        Align2::CENTER_CENTER,
        chip_style(),
        Some(l.chip.width() - px(16.0)),
    );
    // Low-latency output, highlighted: a glow, a tinted plate, a FAST badge before the label, the hint in the
    // highlight's colour
    let col = highlight();
    paint::glow(&p, l.plate, col, 8.0, Some(0.45), Some(px(14.0)));
    p.rect_filled(l.plate, px(14.0), th.base.lerp_to_gamma(col, 0.14));
    p.rect_stroke(
        l.plate,
        px(14.0),
        egui::Stroke::new(1.5, col),
        egui::StrokeKind::Inside,
    );
    p.rect_filled(l.badge, px(8.0), col);
    text::draw(
        &p,
        BADGE,
        l.badge.center(),
        Align2::CENTER_CENTER,
        badge_style(),
        None,
    );
    widgets::draw_switch(&p, l.switch, egui::Id::new((salt, "fastout-sw")), shown.on);
    let row = l.row;
    let top_cy = row.min.y + snap(h_switch() / 2.0);
    let lst = Style::new(15.0, 700, th.text);
    // (room up to the switch: the label is never cut short, Settings had 70 spare)
    let label_max = row.width() - l.switch.width() - px(12.0);
    text::draw(
        &p,
        LABEL,
        pos2(row.min.x, top_cy - px(9.0)),
        Align2::LEFT_CENTER,
        lst,
        Some(label_max),
    );
    let hint_max = row.width() - l.switch.width() - px(20.0);
    for (i, line) in l.hint_lines.iter().enumerate() {
        text::draw(
            &p,
            line,
            pos2(row.min.x, top_cy + px(11.0) + i as f32 * line_h(&ctx, 12.0)),
            Align2::LEFT_CENTER,
            Style::new(12.0, 500, col),
            Some(hint_max),
        );
    }
    if shown.on {
        paint::dot(
            &p,
            pos2(l.plate.max.x - px(8.0), l.plate.min.y + px(8.0)),
            3.5,
            th.accent,
        );
    }
    let resp = interact(ui, row, (salt, "fastout-hit"), egui::Sense::click());
    if resp.hovered() {
        ctx.set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    if resp.clicked() {
        act = Some(Act::Toggle);
    }
    // the buffer: smaller is quicker, bigger rides out a busy computer
    let tr = text::draw(
        &p,
        "Buffer",
        l.buffer_title,
        Align2::LEFT_TOP,
        Style::new(15.0, 700, th.text),
        None,
    );
    if shown.buffer.is_some() {
        paint::dot(&p, pos2(tr.max.x + px(10.0), centre_y(tr)), 3.5, th.accent);
    }
    let (hp, hw) = l.buffer_hint;
    let hst = Style::new(12.0, 500, th.caption);
    let mut hy = hp.y;
    for line in text::wrap(&ctx, BUFFER_HINT, hst, hw) {
        text::draw(&p, &line, pos2(hp.x, hy), Align2::LEFT_TOP, hst, Some(hw));
        hy += line_h(&ctx, 12.0);
    }
    let labels: Vec<&str> = BUFFERS.iter().map(|b| b.0).collect();
    let sel = BUFFERS.iter().position(|b| b.1 == shown.buffer);
    let mut so = Segmented::new(&labels, sel);
    so.size = 13.0;
    so.pad = 4.0;
    so.r = 14.0;
    so.inner_r = 10.0;
    let (_, clicked) = widgets::segmented(ui, l.seg, (salt, "fastout-buffer"), &so);
    if let Some(i) = clicked
        && Some(i) != sel
    {
        act = Some(Act::Buffer(BUFFERS[i].1));
    }
    (l.card, act)
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{Event, PointerButton, Pos2, RawInput, Shape};

    fn shown(on: bool) -> Shown {
        Shown {
            on,
            buffer: Some(128),
            latency: "in 10.6 ms + out 10.6 ms  \u{b7}  WASAPI exclusive".into(),
        }
    }

    fn texts(shape: &Shape, out: &mut Vec<String>) {
        match shape {
            Shape::Vec(v) => v.iter().for_each(|s| texts(s, out)),
            Shape::Text(t) if !t.galley.job.text.trim().is_empty() => out.push(t.galley.job.text.clone()),
            _ => {}
        }
    }

    fn fills(shape: &Shape, out: &mut Vec<Color32>) {
        match shape {
            Shape::Vec(v) => v.iter().for_each(|s| fills(s, out)),
            Shape::Rect(r) => out.push(r.fill),
            _ => {}
        }
    }

    type Drawn = (Layout, Vec<String>, Vec<Color32>, Option<Act>);

    /// The card drawn headless at (40, 30), frame after frame.
    struct Card {
        ctx: egui::Context,
        time: f64,
    }

    impl Card {
        fn new() -> Card {
            let ctx = egui::Context::default();
            text::install(&ctx);
            Card { ctx, time: 0.0 }
        }

        /// One frame: the layout, the texts and fills drawn, what was clicked.
        fn frame(&mut self, frame: Frame, w: f32, s: &Shown, events: Vec<Event>) -> Drawn {
            self.time += 1.0 / 60.0;
            let input = RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1600.0, 1000.0))),
                time: Some(self.time),
                events,
                ..Default::default()
            };
            let mut act = None;
            let mut out = self.ctx.run_ui(input, |ui| {
                act = draw(ui, 40.0, 30.0, w, frame, s, "test").1;
            });
            out.textures_delta.clear();
            let (mut ts, mut fs) = (Vec::new(), Vec::new());
            for cs in &out.shapes {
                texts(&cs.shape, &mut ts);
                fills(&cs.shape, &mut fs);
            }
            (layout(&self.ctx, 40.0, 30.0, w, frame, s), ts, fs, act)
        }

        /// Press and let go at `at`: what the card asked for.
        fn click(&mut self, frame: Frame, w: f32, s: &Shown, at: Pos2) -> Option<Act> {
            let b = |pressed| Event::PointerButton {
                pos: at,
                button: PointerButton::Primary,
                pressed,
                modifiers: Default::default(),
            };
            self.frame(frame, w, s, vec![Event::PointerMoved(at)]);
            self.frame(frame, w, s, vec![b(true)]);
            self.frame(frame, w, s, vec![b(false)]).3
        }
    }

    /// The hint drawn whole, on one line or wrapped over several.
    fn hint_whole(ts: &[String]) -> bool {
        let parts: Vec<&str> = ts
            .iter()
            .filter(|x| HINT.contains(x.as_str()) && x.len() > 4)
            .map(|x| x.as_str())
            .collect();
        parts.join(" ") == HINT
    }

    /// P41.4: Settings' card and the Controls panel's draw the same thing: the same words, whole (nothing cut short
    /// with an ellipsis), the FAST plate's tint and badge, the switch's state; the panel's stacks to fit.
    #[test]
    fn both_places_draw_the_same_card() {
        let mut c = Card::new();
        for on in [false, true] {
            let s = shown(on);
            let (ls, ts, fs, _) = c.frame(Frame::Settings, 1000.0, &s, vec![]);
            let (lp, tp, fp, _) = c.frame(Frame::Panel, 400.0, &s, vec![]);
            assert!(ls.columns && !lp.columns);
            for t_ in [&ts, &tp] {
                assert!(
                    !t_.iter().any(|x| x.contains('\u{2026}')),
                    "nothing cut short: {t_:?}"
                );
                for want in [BADGE, LABEL, "Buffer", "Auto", "128", "512", s.latency.as_str()] {
                    assert!(t_.iter().any(|x| x == want), "{want} drawn: {t_:?}");
                }
                assert!(t_.iter().any(|x| x.contains("REDUCE")), "{t_:?}");
                assert!(hint_whole(t_), "the hint whole: {t_:?}");
            }
            let tint = t().base.lerp_to_gamma(highlight(), 0.14);
            assert!(fs.contains(&tint) && fp.contains(&tint), "the FAST plate in both");
            assert!(
                fs.contains(&highlight()) && fp.contains(&highlight()),
                "the FAST badge in both"
            );
            for l in [&ls, &lp] {
                assert!(
                    l.card.contains_rect(l.plate)
                        && l.card.contains_rect(l.seg)
                        && l.card.contains_rect(l.chip)
                );
                assert!(l.plate.contains_rect(l.switch) && l.plate.contains_rect(l.badge));
                assert!(!l.plate.intersects(l.seg));
            }
        }
        // narrower panels: the chip under the heading, the badge over the label, the hint wrapped; all whole
        for w in [340.0, 300.0] {
            let (l, t_, _, _) = c.frame(Frame::Panel, w, &shown(true), vec![]);
            assert!(l.chip.min.y > l.heading.y + 4.0, "the chip under the heading");
            assert!(
                !t_.iter().any(|x| x.contains('\u{2026}')) && hint_whole(&t_),
                "{w}: {t_:?}"
            );
            assert!(l.card.contains_rect(l.plate) && l.card.contains_rect(l.chip));
            assert!(l.plate.contains_rect(l.badge) && !l.badge.intersects(l.row));
        }
    }

    /// P41.4: a click on the switch or a buffer asks for the same thing in both places, and the player's action
    /// string and Settings' both come back to it (each then calls `apply`).
    #[test]
    fn a_click_asks_the_same_in_both_places() {
        let s = shown(false);
        let mut c = Card::new();
        for (frame, w) in [(Frame::Settings, 1000.0), (Frame::Panel, 400.0)] {
            let l = layout(&c.ctx, 40.0, 30.0, w, frame, &s);
            assert_eq!(
                c.click(frame, w, &s, l.row.center()),
                Some(Act::Toggle),
                "{frame:?}"
            );
            // the segmented's fourth option is 256
            let at = pos2(l.seg.min.x + l.seg.width() * 0.7, l.seg.center().y);
            assert_eq!(
                c.click(frame, w, &s, at),
                Some(Act::Buffer(Some(256))),
                "{frame:?}"
            );
        }
        use crate::player::controls::{fastout_act, fastout_action};
        for a in [Act::Toggle, Act::Buffer(None), Act::Buffer(Some(64))] {
            assert_eq!(Act::parse(&a.action()), Some(a));
            assert_eq!(fastout_act(&fastout_action(a)), Some(a));
        }
        assert_eq!(Act::parse("buffer:auto"), Some(Act::Buffer(None)));
        assert_eq!(
            fastout_act("exclusive"),
            None,
            "(Settings' own string, not the panel's)"
        );
    }

    #[test]
    fn set_switches_and_says_so() {
        let mut p = AudioPrefs::default();
        assert!(!p.exclusive);
        assert!(set(&mut p, Act::Toggle).starts_with("Low-latency output"));
        assert!(p.exclusive);
        assert!(set(&mut p, Act::Toggle).starts_with("Shared output"));
        assert!(!p.exclusive);
        set(&mut p, Act::Buffer(Some(128)));
        assert_eq!(p.buffer, Some(128));
        set(&mut p, Act::Buffer(None));
        assert_eq!(p.buffer, None);
    }

    /// Without a sound engine (or in silent mode) nothing changes, and it says why.
    #[test]
    fn apply_without_an_engine_changes_nothing() {
        let ctx = egui::Context::default();
        let dir = std::env::temp_dir().join(format!("qrs-fastout-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut st = AppState::for_test(&ctx, dir.clone(), Default::default());
        let before = st.config.get("audio").cloned();
        assert_eq!(
            apply(&mut st, Act::Toggle),
            Err("The audio engine isn't running".to_string())
        );
        st.audio_mode = AudioMode::Silent;
        assert!(
            apply(&mut st, Act::Buffer(Some(64)))
                .unwrap_err()
                .starts_with("Silent mode")
        );
        assert_eq!(st.config.get("audio").cloned(), before);
        let _ = std::fs::remove_dir_all(dir);
    }
}
