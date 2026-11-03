//! The score panel of continuous mode (new in quickRS, P29.3, owner request; no Python source): a loop played over
//! and over as a game. Each note judged in the loop scores (hit 100, late or early 50) times a multiplier that
//! climbs a step every `STEP` notes in a row (1x to 4x) and drops back to 1x on a miss; the streak carries from pass
//! to pass. A pass's end adds a bonus: a clean one (90%+) a quarter of its points, a perfect one all of them again,
//! times the run of perfect passes (up to 5). On screen: the multiplier as a badge with a ring filling to the next
//! step, the session's score counting up, "+300"s rising off it, this pass's notes as squares, a chip at each pass's
//! end, and for a perfect pass "PERFECT!", a flash, a ring and confetti that spills out over the tab (fewer from the
//! fourth in a row). Per-note effects stay inside the panel and small (the tab is being read); only a pass's end
//! goes outside it. Timings and sizes from the research brief (PROGRESS, P29.3).

use std::time::Instant;

use eframe::egui;
use egui::{Align2, Color32, Id, LayerId, Order, Pos2, Rect, Stroke, Ui, Vec2, pos2, vec2};
use qrs_analysis::Judgement;
use qrs_ui::paint::{self, Fill};
use qrs_ui::theme::t;
use qrs_ui::units::px;

use super::review::CLEAN;
use super::{PlayerView, snap};
use crate::home::group_digits;
use crate::library::{caps, txt};

/// Notes in a row for each step of the multiplier.
pub const STEP: u32 = 8;
/// The top multiplier.
pub const MAX_MULT: u32 = 4;
/// Perfect passes in a row count up to this for the bonus.
const MAX_CHAIN: u32 = 5;

/// The multiplier for a streak.
pub fn multiplier(streak: u32) -> u32 {
    (1 + streak / STEP).min(MAX_MULT)
}

/// The bonus at a pass's end: (points, what it was) for a pass of `base` unmultiplied points at `acc`, the
/// `chain`th perfect pass in a row (counting this one).
pub fn pass_bonus(base: u64, acc: f64, chain: u32) -> (u64, &'static str) {
    if acc >= 0.9995 {
        (base * chain.clamp(1, MAX_CHAIN) as u64, "perfect")
    } else if acc >= CLEAN {
        (base / 4, "clean")
    } else {
        (0, "")
    }
}

#[derive(Clone, Debug)]
struct Float {
    text: String,
    color: Color32,
    at: Instant,
}

#[derive(Clone, Debug)]
struct Particle {
    pos: Pos2,
    vel: Vec2,
    rot: f32,
    spin: f32,
    flip: f32,
    flip_v: f32,
    size: Vec2,
    color: Color32,
    life: f32,
    age: f32,
    dot: bool,
    wobble: (f32, f32),
}

/// The game over one loop.
#[derive(Default)]
pub struct Arcade {
    /// The loop it's counting (its `loop_key`): another loop starts it again.
    key: Option<(i64, i64)>,
    pub streak: u32,
    pub total: u64,
    pub pass_pts: u64,
    pass_base: u64,
    chain: u32,
    /// The score as shown (counting up to `total`).
    shown: f64,
    floats: Vec<Float>,
    step_at: Option<Instant>,
    miss_at: Option<Instant>,
    /// (text, colour, when) of the last pass's chip.
    chip: Option<(String, Color32, Instant)>,
    perfect_at: Option<Instant>,
    /// Confetti still to throw (counts), from the panel at the next draw.
    burst: u32,
    flash_at: Option<Instant>,
    particles: Vec<Particle>,
    last_draw: Option<Instant>,
    seed: u64,
}

fn secs(since: Option<Instant>) -> Option<f32> {
    since.map(|t| t.elapsed().as_secs_f32())
}

fn ease_out_cubic(x: f32) -> f32 {
    1.0 - (1.0 - x.clamp(0.0, 1.0)).powi(3)
}

/// Overshoot then settle (ease-out-back), 0..1 -> 0..1.
fn ease_out_back(x: f32, s: f32) -> f32 {
    let x = x.clamp(0.0, 1.0) - 1.0;
    1.0 + (s + 1.0) * x * x * x + s * x * x
}

/// Each multiplier's colour (the numeral always says it too).
fn tier_color(m: u32) -> Color32 {
    match m {
        1 => Color32::from_rgb(0x9A, 0xA0, 0xA6),
        2 => Color32::from_rgb(0x38, 0xBD, 0xF8),
        3 => Color32::from_rgb(0x4A, 0xDE, 0x80),
        _ => Color32::from_rgb(0xFB, 0xBF, 0x24),
    }
}

const GOLD: Color32 = Color32::from_rgb(0xFF, 0xD1, 0x66);
const CONFETTI: [Color32; 5] = [
    Color32::from_rgb(0xFF, 0xD1, 0x66),
    Color32::from_rgb(0xEF, 0x47, 0x6F),
    Color32::from_rgb(0x06, 0xD6, 0xA0),
    Color32::from_rgb(0x11, 0x8A, 0xB2),
    Color32::from_rgb(0xF8, 0xF9, 0xFA),
];

impl Arcade {
    /// Count this loop (another loop than the last: from zero).
    pub fn follow(&mut self, key: (i64, i64)) {
        if self.key != Some(key) {
            let seed = self.seed;
            *self = Arcade {
                key: Some(key),
                seed: seed ^ 0x9E37_79B9_7F4A_7C15,
                ..Default::default()
            };
        }
    }

    pub fn mult(&self) -> u32 {
        multiplier(self.streak)
    }

    /// A note of the loop was judged.
    pub fn note(&mut self, j: Judgement) {
        let now = Instant::now();
        if j == Judgement::Miss {
            if self.streak > 0 {
                self.miss_at = Some(now);
            }
            self.streak = 0;
            return;
        }
        let before = self.mult();
        let base = j.points() as u64;
        let pts = base * before as u64;
        self.streak += 1;
        self.pass_pts += pts;
        self.pass_base += base;
        self.total += pts;
        self.floats.push(Float {
            text: format!("+{pts}"),
            color: if j == Judgement::Hit {
                tier_color(before)
            } else {
                tier_color(before).gamma_multiply(0.7)
            },
            at: now,
        });
        if self.floats.len() > 8 {
            self.floats.remove(0);
        }
        if self.mult() > before {
            self.step_at = Some(now);
        }
    }

    /// A pass ended at `acc`; `best_before` the best pass before it.
    pub fn pass(&mut self, acc: f64, best_before: Option<f64>) {
        let now = Instant::now();
        let perfect = acc >= 0.9995;
        self.chain = if perfect { self.chain + 1 } else { 0 };
        let (bonus, kind) = pass_bonus(self.pass_base, acc, self.chain);
        self.total += bonus;
        let pct = (acc * 100.0).round() as i64;
        let new_best = best_before.is_some_and(|b| acc > b + 1e-9);
        let th = t();
        self.chip = Some(match kind {
            "perfect" => (
                if self.chain > 1 {
                    format!("PERFECT ×{}  +{}", self.chain, group_digits(bonus as usize))
                } else {
                    format!("PERFECT  +{}", group_digits(bonus as usize))
                },
                GOLD,
                now,
            ),
            "clean" if new_best => (
                format!("NEW BEST {pct}%  +{}", group_digits(bonus as usize)),
                GOLD,
                now,
            ),
            "clean" => (
                format!("CLEAN {pct}%  +{}", group_digits(bonus as usize)),
                th.hit,
                now,
            ),
            _ if new_best => (format!("NEW BEST {pct}%"), GOLD, now),
            _ => (format!("pass {pct}%"), th.text_2, now),
        });
        if perfect {
            self.perfect_at = Some(now);
            self.flash_at = Some(now);
            // fewer from the fourth in a row: the chain still counts
            let n = (70 + 15 * (self.chain.min(4) - 1)).min(120);
            self.burst = if self.chain >= 4 { n / 2 } else { n };
        }
        self.pass_pts = 0;
        self.pass_base = 0;
    }

    fn rand(&mut self) -> f32 {
        // xorshift64*
        if self.seed == 0 {
            self.seed = 0x2545_F491_4F6C_DD1D;
        }
        let mut x = self.seed;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.seed = x;
        ((x.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 40) as f32) / (1u64 << 24) as f32
    }

    fn throw(&mut self, from: Rect, n: u32, tier: Color32) {
        for k in 0..n {
            let x = from.min.x + from.width() * (k as f32 + self.rand()) / n as f32;
            // up and to the right (away from the playhead), -70° ± 40°
            let ang = (-70.0 + (self.rand() - 0.5) * 80.0_f32).to_radians();
            let speed = px(250.0 + 300.0 * self.rand());
            let dot = self.rand() < 0.2;
            let ci = (self.rand() * 6.0) as usize;
            let q = Particle {
                pos: pos2(x, from.min.y),
                vel: vec2(ang.cos() * speed, ang.sin() * speed),
                rot: self.rand() * std::f32::consts::TAU,
                spin: (4.0 + 8.0 * self.rand()) * if self.rand() < 0.5 { -1.0 } else { 1.0 },
                flip: self.rand() * std::f32::consts::TAU,
                flip_v: 3.0 + 5.0 * self.rand(),
                size: vec2(px(4.0 + 4.0 * self.rand()), px(2.0 + 2.0 * self.rand())),
                color: if ci >= CONFETTI.len() { tier } else { CONFETTI[ci] },
                life: 1.2 + 0.6 * self.rand(),
                age: 0.0,
                dot,
                wobble: (6.0 + 4.0 * self.rand(), self.rand() * std::f32::consts::TAU),
            };
            self.particles.push(q);
        }
    }

    fn step(&mut self, dt: f32, view: Rect) {
        let g = px(900.0);
        for p in &mut self.particles {
            p.age += dt;
            p.vel.y += g * dt;
            p.vel *= (-1.8 * dt).exp();
            let flutter = (p.age * p.wobble.0 + p.wobble.1).sin() * px(30.0);
            p.pos += (p.vel + vec2(flutter, 0.0)) * dt;
            p.rot += p.spin * dt;
            p.flip += p.flip_v * dt;
        }
        self.particles
            .retain(|p| p.age < p.life && view.expand(px(40.0)).contains(p.pos));
        // the count up: closes on the true score in about a quarter second
        let target = self.total as f64;
        self.shown += (target - self.shown) * (1.0 - (-(dt as f64) / 0.07).exp());
        if (target - self.shown).abs() < 1.0 {
            self.shown = target;
        }
        self.floats.retain(|f| f.at.elapsed().as_secs_f32() < 0.45);
    }
}

impl PlayerView {
    /// Newly judged notes of the loop (continuous mode) go to the game.
    pub(super) fn arcade_notes(&mut self, before: &[Option<Judgement>]) {
        if !self.continuous {
            return;
        }
        let Some((a, b)) = self.loop_a.zip(self.loop_b) else {
            return;
        };
        self.arcade.follow(super::continuous::loop_key(a, b));
        for (k, g) in self.scorer.groups.iter().enumerate() {
            if g.t < a || g.t >= b {
                continue;
            }
            if let (Some(j), None) = (g.state, before.get(k).copied().flatten()) {
                self.arcade.note(j);
            }
        }
    }

    /// A pass of the loop ended (continuous mode): its bonus and its chip.
    pub(super) fn arcade_pass(&mut self, acc: f64, best_before: Option<f64>) {
        if let Some((a, b)) = self.loop_a.zip(self.loop_b) {
            self.arcade.follow(super::continuous::loop_key(a, b));
            self.arcade.pass(acc, best_before);
        }
    }

    /// Screenshot mode: a game under way, at 3x, a perfect pass just played.
    pub(super) fn arcade_stage(&mut self) {
        if let Some((a, b)) = self.loop_a.zip(self.loop_b) {
            self.arcade.follow(super::continuous::loop_key(a, b));
        }
        for _ in 0..21 {
            self.arcade.note(Judgement::Hit);
        }
        self.arcade.pass(1.0, Some(0.93));
        for _ in 0..3 {
            self.arcade.note(Judgement::Hit);
        }
        self.arcade.shown = self.arcade.total as f64;
        // (the notes staged as judged aren't news to the game)
        self.states = self.scorer.groups.iter().map(|g| g.state).collect();
        self.arcade.perfect_at = Some(Instant::now() - std::time::Duration::from_millis(350));
        self.arcade.flash_at = None;
    }

    /// The panel, its left edge at `x` and its foot at `bottom`; the confetti over `view`.
    pub(super) fn draw_arcade(&mut self, ui: &Ui, x: f32, bottom: f32, view: Rect) {
        if !self.continuous || self.loop_a.zip(self.loop_b).is_none() {
            self.arcade.particles.clear();
            return;
        }
        let th = t();
        let now = Instant::now();
        let dt = self
            .arcade
            .last_draw
            .map_or(0.016, |l| now.duration_since(l).as_secs_f32().min(0.05));
        self.arcade.last_draw = Some(now);
        self.arcade.step(dt, view);
        let (w, h) = (px(290.0).min(view.max.x - x - px(14.0)), px(130.0));
        if w < px(200.0) {
            return;
        }
        let panel = Rect::from_min_size(pos2(x, bottom - h), vec2(w, h));
        // the panel's little jump at a perfect pass
        let pop = secs(self.arcade.perfect_at)
            .filter(|&s| s < 0.26)
            .map_or(1.0, |s| 1.0 + 0.04 * (s / 0.26 * std::f32::consts::PI).sin());
        let panel_d = Rect::from_center_size(panel.center(), panel.size() * pop);
        let p = ui.painter().clone();
        paint::raised(&p, panel_d, Some(18.0), 6.0, Fill::Base, 0.0);
        let mult = self.arcade.mult();
        let tier = tier_color(mult);
        // the badge: the multiplier, a ring of STEP segments filling to the next step
        let bc = pos2(panel.min.x + px(46.0), panel.min.y + px(52.0));
        let step_s = secs(self.arcade.step_at).filter(|&s| s < 0.4);
        let miss_s = secs(self.arcade.miss_at).filter(|&s| s < 0.25);
        let scale = step_s.map_or(1.0, |s| {
            let k = (s / 0.32).min(1.0);
            1.0 + 0.35 * (1.0 - ease_out_back(k, 1.7)).abs().min(1.0) * (1.0 - k)
                + 0.35 * (k * std::f32::consts::PI).sin() * (1.0 - k)
        });
        let shake = miss_s.map_or(0.0, |s| px(3.0) * (s * 70.0).sin() * (1.0 - s / 0.25));
        let bc = pos2(bc.x + shake, bc.y);
        let badge_col = if miss_s.is_some() { th.unlit } else { tier };
        let r = px(22.0) * scale;
        p.circle_filled(bc, r, badge_col.gamma_multiply(0.9));
        txt(
            &p,
            &format!("{mult}x"),
            bc,
            18.0 * scale,
            Color32::from_rgb(20, 22, 26),
            800,
            Align2::CENTER_CENTER,
            None,
        );
        let ring_r = px(29.0);
        let filled = if mult >= MAX_MULT {
            STEP
        } else {
            self.arcade.streak % STEP
        };
        for k in 0..STEP {
            let a0 = -std::f32::consts::FRAC_PI_2 + k as f32 / STEP as f32 * std::f32::consts::TAU + 0.08;
            let a1 = a0 + std::f32::consts::TAU / STEP as f32 - 0.16;
            let col = if k < filled { tier } else { th.unlit };
            let pts: Vec<Pos2> = (0..=6)
                .map(|q| {
                    let a = a0 + (a1 - a0) * q as f32 / 6.0;
                    pos2(bc.x + ring_r * a.cos(), bc.y + ring_r * a.sin())
                })
                .collect();
            p.add(egui::Shape::line(pts, Stroke::new(px(3.0), col)));
        }
        if mult >= MAX_MULT {
            // at the top: the ring breathes
            let tt = ui.ctx().input(|i| i.time) as f32;
            let glow = 0.25 + 0.2 * (0.5 + 0.5 * (tt * 1.2 * std::f32::consts::TAU).sin());
            p.circle_stroke(
                bc,
                ring_r + px(3.0),
                Stroke::new(px(4.0), tier.gamma_multiply(glow)),
            );
        }
        if let Some(s) = step_s {
            // the step's shock ring and sparks
            let k = ease_out_cubic(s / 0.4);
            p.circle_stroke(
                bc,
                px(24.0) + px(26.0) * k,
                Stroke::new(px(2.0), tier.gamma_multiply(0.6 * (1.0 - k))),
            );
            for q in 0..6 {
                let a = q as f32 / 6.0 * std::f32::consts::TAU + 0.3;
                let d = px(30.0) + px(36.0) * k;
                p.circle_filled(
                    pos2(bc.x + d * a.cos(), bc.y + d * a.sin()),
                    px(2.0),
                    tier.gamma_multiply(1.0 - k),
                );
            }
        }
        // the score, counting up; this pass and the streak under it
        let rx = snap(panel.max.x - px(16.0));
        caps(
            &p,
            "SCORE",
            pos2(rx, panel.min.y + px(12.0)),
            10.0,
            Align2::RIGHT_TOP,
            None,
        );
        let pop_s = self
            .arcade
            .floats
            .last()
            .map(|f| f.at.elapsed().as_secs_f32())
            .filter(|&s| s < 0.12);
        let size = 28.0 * pop_s.map_or(1.0, |s| 1.0 + 0.06 * (1.0 - s / 0.12));
        let sr = txt(
            &p,
            &group_digits(self.arcade.shown.round() as usize),
            pos2(rx, panel.min.y + px(44.0)),
            size,
            th.text,
            800,
            Align2::RIGHT_CENTER,
            None,
        );
        txt(
            &p,
            &format!(
                "pass +{}  ·  streak {}",
                group_digits(self.arcade.pass_pts as usize),
                self.arcade.streak
            ),
            pos2(rx, panel.min.y + px(68.0)),
            12.0,
            if miss_s.is_some() { th.miss } else { th.text_2 },
            700,
            Align2::RIGHT_CENTER,
            None,
        );
        for f in &self.arcade.floats {
            let s = f.at.elapsed().as_secs_f32() / 0.45;
            let k = ease_out_cubic(s);
            txt(
                &p,
                &f.text,
                pos2(sr.min.x - px(8.0), sr.center().y - px(14.0) * k),
                12.0,
                f.color.gamma_multiply(1.0 - k),
                800,
                Align2::RIGHT_CENTER,
                None,
            );
        }
        // this pass's notes, a square each
        let (a, b) = self.loop_a.zip(self.loop_b).unwrap_or_default();
        let marks: Vec<Option<Judgement>> = self
            .scorer
            .groups
            .iter()
            .filter(|g| a <= g.t && g.t < b)
            .map(|g| g.state)
            .collect();
        let sq_row = Rect::from_min_max(
            pos2(panel.min.x + px(14.0), panel.max.y - px(22.0)),
            pos2(panel.max.x - px(14.0), panel.max.y - px(12.0)),
        );
        if !marks.is_empty() {
            let cw = (sq_row.width() / marks.len() as f32).min(px(12.0));
            for (k, m) in marks.iter().enumerate() {
                let c = match m {
                    Some(Judgement::Hit) => th.hit,
                    Some(Judgement::Late | Judgement::Early) => th.late,
                    Some(Judgement::Miss) => th.miss.gamma_multiply(0.7),
                    None => th.unlit,
                };
                p.rect_filled(
                    Rect::from_min_size(
                        pos2(sq_row.min.x + k as f32 * cw, sq_row.min.y),
                        vec2((cw - px(2.0)).max(px(1.0)), sq_row.height()),
                    ),
                    px(2.0),
                    c,
                );
            }
        }
        // the last pass's chip, slid in over the panel's top edge
        if let Some((text_, col, at)) = self.arcade.chip.clone() {
            let s = at.elapsed().as_secs_f32();
            if s < 2.0 {
                let k_in = ease_out_cubic(s / 0.22);
                let alpha = if s > 1.7 { 1.0 - (s - 1.7) / 0.3 } else { 1.0 };
                let tw = qrs_ui::text::width(ui.ctx(), &text_, qrs_ui::text::Style::new(12.0, 800, col));
                let cw = tw + px(20.0);
                let cx = panel.max.x - px(10.0) - cw + px(40.0) * (1.0 - k_in);
                let chip = Rect::from_min_size(pos2(cx, panel.min.y - px(12.0)), vec2(cw, px(22.0)));
                p.rect_filled(chip, px(11.0), th.base.gamma_multiply(alpha));
                p.rect_stroke(
                    chip,
                    px(11.0),
                    Stroke::new(px(1.5), col.gamma_multiply(alpha)),
                    egui::StrokeKind::Inside,
                );
                txt(
                    &p,
                    &text_,
                    chip.center(),
                    12.0,
                    col.gamma_multiply(alpha),
                    800,
                    Align2::CENTER_CENTER,
                    None,
                );
            }
        }
        // a perfect pass: a flash, a ring out of the panel, "PERFECT!", confetti
        if let Some(s) = secs(self.arcade.flash_at).filter(|&s| s < 0.18) {
            p.rect_filled(
                panel_d,
                px(18.0),
                Color32::WHITE.gamma_multiply(0.35 * (1.0 - s / 0.18)),
            );
        }
        let top = ui
            .ctx()
            .layer_painter(LayerId::new(Order::Foreground, Id::new("arcade-fx")));
        let top = top.with_clip_rect(view);
        if let Some(s) = secs(self.arcade.perfect_at).filter(|&s| s < 1.6) {
            if s < 0.45 && self.arcade.chain < 4 {
                let k = ease_out_cubic(s / 0.45);
                top.circle_stroke(
                    panel.center(),
                    px(160.0) * k,
                    Stroke::new(px(3.0), GOLD.gamma_multiply(0.5 * (1.0 - k))),
                );
            }
            let scale = if s < 0.16 {
                0.4 + 0.75 * (s / 0.16)
            } else if s < 0.28 {
                1.15 - 0.15 * ((s - 0.16) / 0.12)
            } else {
                1.0
            };
            let (rise, alpha) = if s > 1.3 {
                (px(10.0) * (s - 1.3) / 0.3, 1.0 - (s - 1.3) / 0.3)
            } else {
                (0.0, 1.0)
            };
            let label = if self.arcade.chain > 1 {
                format!("PERFECT ×{}", self.arcade.chain)
            } else {
                "PERFECT!".into()
            };
            let at = pos2(panel.center().x, panel.min.y - px(30.0) - rise);
            for (dx, dy) in [(-1.0, 0.0), (1.0, 0.0), (0.0, -1.0), (0.0, 1.0)] {
                txt(
                    &top,
                    &label,
                    pos2(at.x + px(dx), at.y + px(dy)),
                    30.0 * scale,
                    Color32::from_black_alpha((200.0 * alpha) as u8),
                    800,
                    Align2::CENTER_CENTER,
                    None,
                );
            }
            txt(
                &top,
                &label,
                at,
                30.0 * scale,
                GOLD.gamma_multiply(alpha),
                800,
                Align2::CENTER_CENTER,
                None,
            );
        }
        if self.arcade.burst > 0 {
            let n = std::mem::take(&mut self.arcade.burst);
            self.arcade.throw(panel, n, tier);
            // (a second puff from the badge comes with the first here: one frame, no timer)
            let puff = Rect::from_center_size(bc, Vec2::splat(px(20.0)));
            self.arcade.throw(puff, 25, tier);
        }
        for q in &self.arcade.particles {
            let fade = ((q.life - q.age) / 0.3).clamp(0.0, 1.0);
            let col = q.color.gamma_multiply(fade);
            if q.dot {
                top.circle_filled(q.pos, px(2.0), col);
            } else {
                let w = q.size.x * q.flip.cos().abs().max(0.15);
                let (c, s) = (q.rot.cos(), q.rot.sin());
                let corners = [(-w, -q.size.y), (w, -q.size.y), (w, q.size.y), (-w, q.size.y)]
                    .map(|(x, y)| pos2(q.pos.x + (x * c - y * s) / 2.0, q.pos.y + (x * s + y * c) / 2.0));
                top.add(egui::Shape::convex_polygon(corners.to_vec(), col, Stroke::NONE));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_multiplier_steps_every_eight_notes_up_to_four() {
        assert_eq!(multiplier(0), 1);
        assert_eq!(multiplier(7), 1);
        assert_eq!(multiplier(8), 2);
        assert_eq!(multiplier(23), 3);
        assert_eq!(multiplier(24), 4);
        assert_eq!(multiplier(500), 4);
    }

    #[test]
    fn a_note_scores_times_the_multiplier_and_a_miss_resets_it() {
        let mut a = Arcade::default();
        for _ in 0..8 {
            a.note(Judgement::Hit);
        }
        assert_eq!(a.total, 800);
        assert_eq!(a.mult(), 2);
        a.note(Judgement::Late);
        assert_eq!(a.total, 900); // 50 x 2
        a.note(Judgement::Miss);
        assert_eq!((a.streak, a.mult()), (0, 1));
    }

    #[test]
    fn the_bonus_for_clean_and_perfect_passes() {
        assert_eq!(pass_bonus(1000, 0.95, 0), (250, "clean"));
        assert_eq!(pass_bonus(1000, 1.0, 1), (1000, "perfect"));
        assert_eq!(pass_bonus(1000, 1.0, 3), (3000, "perfect"));
        assert_eq!(pass_bonus(1000, 1.0, 9), (5000, "perfect"));
        assert_eq!(pass_bonus(1000, 0.8, 0), (0, ""));
    }

    #[test]
    fn perfect_passes_chain_and_throw_confetti() {
        let mut a = Arcade::default();
        for _ in 0..4 {
            a.note(Judgement::Hit);
        }
        a.pass(1.0, None);
        assert_eq!(a.chain, 1);
        assert!(a.burst >= 70);
        assert_eq!(a.total, 800); // 400 played + 400 bonus
        a.pass(0.8, None);
        assert_eq!(a.chain, 0);
    }

    #[test]
    fn another_loop_starts_from_zero() {
        let mut a = Arcade::default();
        a.follow((0, 1000));
        a.note(Judgement::Hit);
        a.follow((0, 1000));
        assert_eq!(a.total, 100);
        a.follow((0, 2000));
        assert_eq!(a.total, 0);
    }
}
