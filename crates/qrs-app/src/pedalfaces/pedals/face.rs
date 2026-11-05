//! A face from its spec (PLAN.md P21.3): where every control sits on the real unit (in the mockups' units,
//! 2.2 px a millimetre), how its body and print are drawn. `layout`, `body` and `controls` here serve every pedal,
//! console, rack unit and graphic EQ; each model only says what's where and paints its own art.

use eframe::egui::{Align2, Color32, Painter, Pos2, Rect, Stroke, Vec2, pos2, vec2};
use qrs_audio::board::{Block, KnobDef, Unit};
use qrs_ui::raster::Image;
use qrs_ui::text::{self, Style};
use qrs_ui::theme::t;

use super::super::{Detail, Hot, Layout, Lettering};
use super::kit::{self, C, Cv, Fit, Ks, c32, c32a};

/// Where a control's label goes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum LabPos {
    Above,
    Below,
    Right,
    /// At a point (mockup units), centred.
    At(f32, f32),
    None,
}

/// A control's printed label.
#[derive(Clone, Copy, Debug)]
pub struct Lab {
    pub pos: LabPos,
    /// Mockup units.
    pub size: f32,
    pub col: C,
    pub style: Lettering,
    /// The text, when not the knob's own label.
    pub text: &'static str,
    /// The gap between the control and its label (mockup units).
    pub gap: f32,
}

impl Lab {
    pub fn new(pos: LabPos, size: f32, col: C) -> Lab {
        Lab {
            pos,
            size,
            col,
            style: Lettering::Condensed,
            text: "",
            gap: 4.0,
        }
    }

    pub fn none() -> Lab {
        Lab::new(LabPos::None, 0.0, 0)
    }

    pub fn text(mut self, s: &'static str) -> Lab {
        self.text = s;
        self
    }

    pub fn style(mut self, s: Lettering) -> Lab {
        self.style = s;
        self
    }

    pub fn gap(mut self, g: f32) -> Lab {
        self.gap = g;
        self
    }
}

/// A knob on the face: the model's knob `i`.
#[derive(Clone, Copy, Debug)]
pub struct Kn {
    pub i: usize,
    pub x: f32,
    pub y: f32,
    pub r: f32,
    pub s: Ks,
    pub lab: Lab,
    /// A printed scale round it: (ticks, colour); 0 = none.
    pub ticks: (u32, C),
}

pub fn kn(i: usize, x: f32, y: f32, r: f32, s: Ks, lab: Lab) -> Kn {
    Kn {
        i,
        x,
        y,
        r,
        s,
        lab,
        ticks: (0, 0),
    }
}

/// How a switch with a few positions looks.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ChK {
    /// A bat-handle toggle leaning towards the chosen side; the options printed either side.
    Toggle,
    /// A knob whose positions spread left to right round its top.
    Rotary(Ks),
    /// The half-moon lever of an organ speaker's speed switch.
    HalfMoon,
    /// Drawn by the model's own art (a radio's tuning dial).
    Art,
}

/// A switch: the model's knob `i`, in a box (mockup units) split left to right among its options.
#[derive(Clone, Copy, Debug)]
pub struct Chs {
    pub i: usize,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub k: ChK,
    pub lab: Lab,
    /// The options' print (short), when not the knob's own.
    pub opts: &'static [&'static str],
    pub ink: C,
}

/// A slider: the model's knob `i`, its slot from y0 (top) to y1.
#[derive(Clone, Copy, Debug)]
pub struct Fd {
    pub i: usize,
    pub x: f32,
    pub y0: f32,
    pub y1: f32,
    /// The cap's size (mockup units).
    pub cap: Vec2,
    /// A chrome ribbed cap (else a small black one with a white line).
    pub chrome: bool,
    pub lab: Lab,
}

/// The light that's lit when the block is on.
#[derive(Clone, Copy, Debug)]
pub struct Led {
    pub x: f32,
    pub y: f32,
    pub r: f32,
    pub c: C,
    /// A faceted jewel lamp (else an LED).
    pub jewel: bool,
    /// A chrome bezel round an LED.
    pub bezel: bool,
}

pub const RED: C = 0xff3b30;

pub fn led(x: f32, y: f32, r: f32) -> Option<Led> {
    Some(Led {
        x,
        y,
        r,
        c: RED,
        jewel: false,
        bezel: true,
    })
}

/// What turns the block on and off.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Sw {
    None,
    /// A chrome footswitch.
    Stomp(f32, f32, f32),
    /// An area drawn by the body (a hinged pedal plate, a bar switch).
    Area(f32, f32, f32, f32),
    /// A bat toggle, up when on (x, y, r).
    Toggle(f32, f32, f32),
    /// An on/off slide switch (x, y, w, h).
    Slide(f32, f32, f32, f32),
    /// A lit push key (x, y, w, h) with its print.
    Key(f32, f32, f32, f32, &'static str),
}

/// A model's colours and print, for its template's painters.
#[derive(Clone, Copy, Debug)]
pub struct Look {
    /// The enclosure's colour.
    pub c: C,
    /// The print's colour.
    pub ink: C,
    /// The name as printed.
    pub name: &'static str,
    /// What it is, in small print ("BLUES OVERDRIVE").
    pub sub: &'static str,
    /// The name's colour.
    pub accent: C,
    /// The name's lettering.
    pub style: Lettering,
    pub finish: kit::Finish,
}

impl Default for Look {
    fn default() -> Self {
        Look {
            c: 0x808080,
            ink: 0x111111,
            name: "",
            sub: "",
            accent: 0xffffff,
            style: Lettering::Condensed,
            finish: kit::Finish::Gloss,
        }
    }
}

/// Everything about one model's face.
pub struct Spec {
    pub id: &'static str,
    pub look: Look,
    /// The enclosure's size (mockup units).
    pub w: f32,
    pub h: f32,
    /// What sticks out (jacks, fins, a cable): left, top, right, bottom.
    pub ext: [f32; 4],
    /// The corner radius of its shadow (mockup units).
    pub radius: f32,
    /// A console or rack: wider on the board.
    pub wide: bool,
    pub knobs: Vec<Kn>,
    pub choices: Vec<Chs>,
    pub faders: Vec<Fd>,
    pub led: Option<Led>,
    pub switch: Sw,
    /// The static art: the enclosure and everything printed into it that isn't text.
    pub body: fn(&mut Cv, &Spec),
    /// The live art: the print, and anything that moves with a value or the on state.
    pub art: fn(&Pr),
}

impl Spec {
    pub fn new(id: &'static str, w: f32, h: f32, body: fn(&mut Cv, &Spec), art: fn(&Pr)) -> Spec {
        Spec {
            id,
            look: Look::default(),
            w,
            h,
            ext: [0.0; 4],
            radius: 8.0,
            wide: false,
            knobs: Vec::new(),
            choices: Vec::new(),
            faders: Vec::new(),
            led: None,
            switch: Sw::None,
            body,
            art,
        }
    }

    pub fn fit(&self, r: Rect) -> Fit {
        Fit::new(r, self.w, self.h, self.ext)
    }
}

/// What the live art gets.
pub struct Pr<'a> {
    pub p: &'a Painter,
    pub f: Fit,
    pub b: &'a Block,
    pub detail: Detail,
    pub spec: &'a Spec,
}

impl Pr<'_> {
    /// The smallest print worth drawing (points): labels on the board are tiny, in the editor readable.
    pub fn min(&self) -> f32 {
        if self.detail == Detail::Board { 4.0 } else { 6.5 }
    }

    /// Print at mockup coordinates.
    #[allow(clippy::too_many_arguments)]
    pub fn t(
        &self,
        s: &str,
        x: f32,
        y: f32,
        anchor: Align2,
        size: f32,
        c: C,
        style: Lettering,
        max_w: f32,
    ) -> Rect {
        kit::print(
            self.p,
            &self.f,
            s,
            x,
            y,
            anchor,
            size,
            c,
            style,
            max_w,
            self.min(),
        )
    }

    /// Print keeping its case (scripts, lowercase logos).
    #[allow(clippy::too_many_arguments)]
    pub fn ta(
        &self,
        s: &str,
        x: f32,
        y: f32,
        anchor: Align2,
        size: f32,
        c: C,
        style: Lettering,
        max_w: f32,
    ) -> Rect {
        kit::print_as(
            self.p,
            &self.f,
            s,
            x,
            y,
            anchor,
            size,
            c,
            style,
            max_w,
            self.min(),
        )
    }

    /// Print rotated `deg` degrees about its centre.
    #[allow(clippy::too_many_arguments)]
    pub fn rot(&self, s: &str, x: f32, y: f32, size: f32, c: C, style: Lettering, deg: f32) {
        kit::text_rot(
            self.p,
            s,
            self.f.p(x, y),
            self.f.s(size).max(self.min()),
            c32(c),
            style,
            deg.to_radians(),
        );
    }

    /// Plain print in the app's own face (lowercase small print), size in mockup units.
    #[allow(clippy::too_many_arguments)]
    pub fn plain(&self, s: &str, x: f32, y: f32, anchor: Align2, size: f32, c: C, weight: u16) -> Rect {
        // (the app's text has a floor of 8 pixels: smaller print than that would spill, so it's left off)
        let px = self.f.s(size) * self.p.pixels_per_point();
        if px < 8.0 {
            return Rect::NOTHING;
        }
        text::draw(
            self.p,
            s,
            self.f.p(x, y),
            anchor,
            Style::new(self.f.s(size).max(self.min()), weight, c32(c)),
            None,
        )
    }

    pub fn on(&self) -> bool {
        self.b.on
    }

    /// The turn (0..1) of the model's knob `i`.
    pub fn u(&self, i: usize) -> f32 {
        let def = self.b.def();
        def.knobs.get(i).map(|k| k.to_u(self.b.v[i])).unwrap_or(0.0)
    }
}

// -- layout -------------------------------------------------------------------------------------------------------------

pub fn layout(s: &Spec, r: Rect) -> Layout {
    let f = s.fit(r);
    let mut l = Layout {
        radius: f.s(s.radius),
        panel: f.rect(0.0, 0.0, s.w, s.h),
        ..Default::default()
    };
    for k in &s.knobs {
        l.knobs.push((k.i, f.p(k.x, k.y), f.s(k.r * 2.0)));
    }
    for c in &s.choices {
        l.choices.push((c.i, f.rect(c.x, c.y, c.w, c.h)));
    }
    for fd in &s.faders {
        l.faders.push((
            fd.i,
            Rect::from_min_max(f.p(fd.x - 3.0, fd.y0), f.p(fd.x + 3.0, fd.y1)),
        ));
    }
    if let Some(led) = s.led {
        l.led = f.p(led.x, led.y);
    }
    l.switch = match s.switch {
        Sw::None => Rect::NOTHING,
        Sw::Stomp(x, y, rr) => Rect::from_center_size(f.p(x, y), Vec2::splat(f.s(rr * 2.0))),
        Sw::Toggle(x, y, rr) => {
            Rect::from_center_size(f.p(x, y - rr * 0.6), vec2(f.s(rr * 2.4), f.s(rr * 3.6)))
        }
        Sw::Area(x, y, w, h) | Sw::Slide(x, y, w, h) | Sw::Key(x, y, w, h, _) => f.rect(x, y, w, h),
    };
    l.name = l.panel;
    l
}

/// The size on the board: the real unit's proportions at about 1.47 points a millimetre, no taller than 200
/// points, no wider than 260 (a console or rack 300).
pub fn board_size(s: &Spec) -> Vec2 {
    let w = (s.w + s.ext[0] + s.ext[2]) * 0.668;
    let h = (s.h + s.ext[1] + s.ext[3]) * 0.668;
    let max_w = if s.wide { 300.0 } else { 260.0 };
    let k = (200.0 / h).min(max_w / w).min(1.0);
    vec2(w * k, h * k)
}

// -- body ---------------------------------------------------------------------------------------------------------------

pub fn body(s: &Spec, r: Rect, ppp: f32) -> Image {
    body_as(s, r, ppp, true)
}

/// The body without its knobs (their caps and printed scales): a miniature's, where each knob is a dot (P37.12).
pub fn body_bare(s: &Spec, r: Rect, ppp: f32) -> Image {
    body_as(s, r, ppp, false)
}

fn body_as(s: &Spec, r: Rect, ppp: f32, knobs: bool) -> Image {
    let f = s.fit(r);
    let Some(mut cv) = Cv::new(r, &f, ppp) else {
        return Image::new(1, 1);
    };
    (s.body)(&mut cv, s);
    for k in s.knobs.iter().filter(|_| knobs) {
        if k.ticks.0 > 1 {
            let n = k.ticks.0;
            for j in 0..n {
                let a = -135.0 + 270.0 * j as f32 / (n - 1) as f32;
                let major = j == 0 || j == n - 1 || (n == 11 && j == 5);
                let (x0, y0) = kit::polar(k.x, k.y, k.r + 3.0, a);
                let (x1, y1) = kit::polar(k.x, k.y, k.r + if major { 7.5 } else { 5.5 }, a);
                cv.line(
                    x0,
                    y0,
                    x1,
                    y1,
                    &kit::col(k.ticks.1, 0.9),
                    if major { 1.2 } else { 0.8 },
                );
            }
        }
        kit::knob_body(&mut cv, k.s, k.x, k.y, k.r);
    }
    for c in &s.choices {
        match c.k {
            ChK::Rotary(_) if !knobs => {}
            ChK::Rotary(ks) => {
                let rr = c.w.min(c.h) * 0.36;
                let (cx, cy) = (c.x + c.w / 2.0, c.y + c.h * 0.58);
                kit::knob_body(&mut cv, ks, cx, cy, rr);
            }
            ChK::Toggle => {
                let (cx, cy) = (c.x + c.w / 2.0, c.y + c.h * 0.6);
                toggle_nut(&mut cv, cx, cy, c.h * 0.2);
            }
            ChK::HalfMoon => {
                let (cx, cy) = (c.x + c.w / 2.0, c.y + c.h * 0.78);
                cv.rect(
                    cx - c.w * 0.42,
                    cy,
                    c.w * 0.84,
                    3.0,
                    1.5,
                    &kit::col(0x0c0806, 1.0),
                );
            }
            ChK::Art => {}
        }
    }
    if let Some(led) = s.led {
        if led.jewel {
            kit::jewel_bezel(&mut cv, led.x, led.y, led.r);
        } else if led.bezel {
            kit::led_bezel(&mut cv, led.x, led.y, led.r);
        }
    }
    match s.switch {
        Sw::Stomp(x, y, rr) => kit::stomp(&mut cv, x, y, rr),
        Sw::Toggle(x, y, rr) => toggle_nut(&mut cv, x, y, rr),
        Sw::Slide(x, y, w, h) => {
            cv.rect(x, y + h * 0.2, w, h * 0.6, h * 0.3, &kit::col(0x0d0e10, 1.0));
        }
        _ => {}
    }
    cv.image()
}

/// A toggle's hex nut (the lever is drawn live).
pub fn toggle_nut(cv: &mut Cv, cx: f32, cy: f32, r: f32) {
    kit::kshadow(cv, cx, cy, r, 0.45);
    let hex: Vec<(f32, f32)> = (0..6)
        .map(|i| {
            let a = (60.0 * i as f32).to_radians();
            (cx + r * a.cos(), cy + r * a.sin())
        })
        .collect();
    if let Some(p) = kit::poly(&hex) {
        cv.fill(&p, &kit::chrome(cx - r, cy - r, 2.0 * r, 2.0 * r));
        cv.stroke(&p, &kit::col(0x43484e, 1.0), 0.6);
    }
    cv.circle(cx, cy, r * 0.55, &kit::col(0x4d5258, 1.0));
    cv.circle(cx, cy, r * 0.4, &kit::col(0x1d1f22, 1.0));
}

/// A toggle's chrome lever, leaning by `lean` (-1 left .. 1 right; up is lean 0 with `up`).
pub fn lever(p: &Painter, f: &Fit, cx: f32, cy: f32, r: f32, deg: f32) {
    let (tx, ty) = kit::polar(cx, cy, r * 1.6, deg);
    let shadow = vec2(f.s(r * 0.25), f.s(r * 0.3));
    p.line_segment(
        [f.p(cx, cy) + shadow, f.p(tx, ty) + shadow],
        Stroke::new(f.s(r * 0.5).max(1.2), Color32::from_black_alpha(80)),
    );
    p.line_segment(
        [f.p(cx, cy), f.p(tx, ty)],
        Stroke::new(f.s(r * 0.48).max(1.4), Color32::from_rgb(70, 75, 82)),
    );
    p.line_segment(
        [f.p(cx, cy), f.p(tx, ty)],
        Stroke::new(f.s(r * 0.26).max(0.8), Color32::from_rgb(236, 238, 242)),
    );
    p.circle_filled(
        f.p(tx, ty),
        f.s(r * 0.34).max(1.2),
        Color32::from_rgb(214, 218, 223),
    );
    p.circle_filled(
        f.p(tx - r * 0.1, ty - r * 0.1),
        f.s(r * 0.16).max(0.6),
        Color32::WHITE,
    );
}

// -- controls -------------------------------------------------------------------------------------------------------------

fn label_text(lab: &Lab, kd: &KnobDef) -> String {
    if lab.text.is_empty() {
        kd.label.to_string()
    } else {
        lab.text.to_string()
    }
}

/// Where a label goes for a control at (x, y) of radius `r`.
fn label_at(lab: &Lab, x: f32, y: f32, r: f32) -> Option<(f32, f32, Align2)> {
    let g = lab.gap;
    Some(match lab.pos {
        LabPos::Above => (x, y - r - g, Align2::CENTER_BOTTOM),
        LabPos::Below => (x, y + r + g, Align2::CENTER_TOP),
        LabPos::Right => (x + r + g, y, Align2::LEFT_CENTER),
        LabPos::At(ax, ay) => (ax, ay, Align2::CENTER_CENTER),
        LabPos::None => return None,
    })
}

/// A value shown while it's turned: a dark bubble.
fn bubble(p: &Painter, at: Pos2, s: &str, detail: Detail) {
    let size = if detail == Detail::Board { 9.0 } else { 11.0 };
    let w = text::width(p.ctx(), s, Style::new(size, 800, Color32::WHITE));
    let r = Rect::from_center_size(at, vec2(w + 10.0, size + 7.0));
    p.rect_filled(
        r.translate(vec2(0.0, 1.0)),
        r.height() * 0.5,
        Color32::from_black_alpha(90),
    );
    p.rect_filled(
        r,
        r.height() * 0.5,
        Color32::from_rgba_unmultiplied(16, 17, 19, 235),
    );
    p.rect_stroke(
        r,
        r.height() * 0.5,
        Stroke::new(1.0, t().accent),
        eframe::egui::StrokeKind::Inside,
    );
    text::draw(
        p,
        s,
        r.center(),
        Align2::CENTER_CENTER,
        Style::new(size, 800, Color32::WHITE),
        None,
    );
}

pub fn controls(s: &Spec, p: &Painter, r: Rect, b: &Block, detail: Detail, hot: Hot) {
    let f = s.fit(r);
    let def = b.def();
    let th = t();
    let pr = Pr {
        p,
        f,
        b,
        detail,
        spec: s,
    };
    (s.art)(&pr);
    let min = pr.min();
    let mut bubbles: Vec<(Pos2, String)> = Vec::new();
    // the room a label has: up to its neighbours in the row
    let room = |x: f32, y: f32, r: f32| -> f32 {
        let gap = s
            .knobs
            .iter()
            .filter(|o| (o.y - y).abs() < r && (o.x - x).abs() > 0.5)
            .map(|o| (o.x - x).abs())
            .fold(f32::INFINITY, f32::min);
        (r * 2.0 + 30.0).min(gap - 3.0).max(10.0)
    };
    for k in &s.knobs {
        let Some(kd) = def.knobs.get(k.i) else { continue };
        let u = kd.to_u(b.v[k.i]);
        kit::pointer(p, &f, k.s, k.x, k.y, k.r, u);
        if let Some((lx, ly, anchor)) = label_at(&k.lab, k.x, k.y, k.r) {
            let w = match k.lab.pos {
                LabPos::Right => 80.0,
                _ => room(k.x, k.y, k.r),
            };
            kit::print(
                p,
                &f,
                &label_text(&k.lab, kd),
                lx,
                ly,
                anchor,
                k.lab.size,
                k.lab.col,
                k.lab.style,
                w,
                min,
            );
        }
        if hot.hover == Some(k.i) && hot.active != Some(k.i) && k.s != Ks::Hidden {
            p.circle_stroke(
                f.p(k.x, k.y),
                f.s(k.r) + 2.5,
                Stroke::new(1.3, th.accent.gamma_multiply(0.8)),
            );
        }
        if hot.active == Some(k.i) || hot.hover == Some(k.i) {
            let at = f.p(k.x, k.y + k.r) + vec2(0.0, 12.0);
            bubbles.push((at, kd.text(b.v[k.i])));
        }
    }
    for c in &s.choices {
        let Some(kd) = def.knobs.get(c.i) else { continue };
        choice(&pr, c, kd, b.v[c.i], hot.hover == Some(c.i));
    }
    for fd in &s.faders {
        let Some(kd) = def.knobs.get(fd.i) else { continue };
        let u = kd.to_u(b.v[fd.i]);
        let y = fd.y1 - (fd.y1 - fd.y0) * u;
        fader_cap(p, &f, fd, y);
        if let Some((lx, ly, anchor)) = label_at(&fd.lab, fd.x, fd.y1, 0.0) {
            kit::print(
                p,
                &f,
                &label_text(&fd.lab, kd),
                lx,
                ly,
                anchor,
                fd.lab.size,
                fd.lab.col,
                fd.lab.style,
                40.0,
                min,
            );
        }
        let hov = hot.hover == Some(fd.i) || hot.active == Some(fd.i);
        if hov {
            let cap = Rect::from_center_size(f.p(fd.x, y), vec2(f.s(fd.cap.x), f.s(fd.cap.y)));
            p.rect_stroke(
                cap.expand(1.5),
                2.5,
                Stroke::new(1.3, th.accent),
                eframe::egui::StrokeKind::Outside,
            );
            bubbles.push((f.p(fd.x, fd.y0) - vec2(0.0, 12.0), kd.text(b.v[fd.i])));
        }
    }
    if let Some(led) = s.led {
        if led.jewel {
            kit::jewel_lens(p, &f, led.x, led.y, led.r, led.c, b.on);
        } else {
            kit::led_lens(p, &f, led.x, led.y, led.r, led.c, b.on);
        }
    }
    match s.switch {
        Sw::Toggle(x, y, rr) => lever(p, &f, x, y, rr, if b.on { 0.0 } else { 180.0 }),
        Sw::Slide(x, y, w, h) => {
            let sw = h * 1.6;
            let sx = if b.on { x + w - sw } else { x };
            let rr = f.rect(sx, y - h * 0.1, sw, h * 1.2);
            kit::vgrad(
                p,
                rr,
                Color32::from_rgb(244, 246, 248),
                Color32::from_rgb(140, 146, 154),
            );
            for i in 0..4 {
                let xx = sx + sw * (0.22 + 0.19 * i as f32);
                p.line_segment(
                    [f.p(xx, y + h * 0.1), f.p(xx, y + h * 0.9)],
                    Stroke::new(f.s(0.8).max(0.5), Color32::from_rgb(75, 80, 86)),
                );
            }
        }
        Sw::Key(x, y, w, h, txt) => {
            let rr = f.rect(x, y, w, h);
            p.rect_filled(
                rr.translate(vec2(0.0, f.s(3.0))),
                3.0,
                Color32::from_rgb(107, 113, 120),
            );
            kit::vgrad(
                p,
                rr,
                Color32::from_rgb(244, 245, 246),
                Color32::from_rgb(195, 200, 204),
            );
            pr.t(
                txt,
                x + w / 2.0,
                y + h / 2.0,
                Align2::CENTER_CENTER,
                h * 0.42,
                0x1e2124,
                Lettering::Condensed,
                w,
            );
        }
        _ => {}
    }
    if hot.switch {
        let sw = r_of_switch(s, &f);
        match s.switch {
            Sw::Stomp(..) => {
                p.circle_stroke(sw.center(), sw.width() * 0.5 + 3.0, Stroke::new(1.5, th.accent));
            }
            Sw::None => {}
            _ => {
                p.rect_stroke(
                    sw.expand(2.0),
                    4.0,
                    Stroke::new(1.5, th.accent),
                    eframe::egui::StrokeKind::Outside,
                );
            }
        }
    }
    for (at, s) in bubbles {
        bubble(p, at, &s, detail);
    }
}

fn r_of_switch(s: &Spec, f: &Fit) -> Rect {
    match s.switch {
        Sw::None => Rect::NOTHING,
        Sw::Stomp(x, y, rr) => Rect::from_center_size(f.p(x, y), Vec2::splat(f.s(rr * 2.0))),
        Sw::Toggle(x, y, rr) => {
            Rect::from_center_size(f.p(x, y - rr * 0.6), vec2(f.s(rr * 2.4), f.s(rr * 3.6)))
        }
        Sw::Area(x, y, w, h) | Sw::Slide(x, y, w, h) | Sw::Key(x, y, w, h, _) => f.rect(x, y, w, h),
    }
}

/// A slider's cap at `y`.
fn fader_cap(p: &Painter, f: &Fit, fd: &Fd, y: f32) {
    let cap = Rect::from_center_size(f.p(fd.x, y), vec2(f.s(fd.cap.x), f.s(fd.cap.y)));
    p.rect_filled(
        cap.translate(vec2(0.0, f.s(2.0))),
        2.0,
        Color32::from_black_alpha(130),
    );
    if fd.chrome {
        kit::vgrad(p, cap, Color32::WHITE, Color32::from_rgb(141, 147, 153));
        let n = 5;
        for i in 0..n {
            let yy = cap.min.y + cap.height() * (i as f32 + 0.5) / n as f32;
            p.line_segment(
                [pos2(cap.min.x + 1.0, yy), pos2(cap.max.x - 1.0, yy)],
                Stroke::new(0.7, Color32::from_rgb(154, 160, 166)),
            );
        }
    } else {
        kit::vgrad(
            p,
            cap,
            Color32::from_rgb(58, 58, 58),
            Color32::from_rgb(14, 14, 14),
        );
        p.line_segment(
            [
                pos2(cap.min.x + 1.0, cap.center().y),
                pos2(cap.max.x - 1.0, cap.center().y),
            ],
            Stroke::new(f.s(1.5).clamp(0.8, 1.6), Color32::from_rgb(233, 233, 233)),
        );
    }
}

/// A switch with a few positions.
fn choice(pr: &Pr, c: &Chs, kd: &KnobDef, v: f32, hot: bool) {
    let Unit::Choice(names) = kd.unit else { return };
    let (p, f) = (pr.p, &pr.f);
    let n = names.len().max(1);
    let sel = (v.round().max(0.0) as usize).min(n - 1);
    let opt = |k: usize| -> String {
        c.opts
            .get(k)
            .map(|s| s.to_string())
            .unwrap_or_else(|| names[k].to_uppercase())
    };
    let osize = (c.lab.size * 0.85).max(4.0);
    match c.k {
        ChK::Toggle => {
            let (cx, cy) = (c.x + c.w / 2.0, c.y + c.h * 0.6);
            let rr = c.h * 0.2;
            let deg = if n <= 1 {
                0.0
            } else {
                -40.0 + 80.0 * sel as f32 / (n - 1) as f32
            };
            lever(p, f, cx, cy, rr, deg);
            for k in 0..n {
                let x = if n == 1 {
                    cx
                } else {
                    c.x + c.w * (k as f32 / (n - 1) as f32)
                };
                let anchor = if k == 0 {
                    Align2::LEFT_CENTER
                } else if k == n - 1 {
                    Align2::RIGHT_CENTER
                } else {
                    Align2::CENTER_CENTER
                };
                let col = if k == sel {
                    c.ink
                } else {
                    kit::mix(c.ink, 0x808080, 0.5)
                };
                pr.t(
                    &opt(k),
                    x,
                    c.y + c.h * 0.95,
                    anchor,
                    osize,
                    col,
                    c.lab.style,
                    c.w * 0.45,
                );
            }
        }
        ChK::Rotary(ks) => {
            let rr = c.w.min(c.h) * 0.36;
            let (cx, cy) = (c.x + c.w / 2.0, c.y + c.h * 0.58);
            let spread = if n <= 1 { 0.0 } else { 100.0 };
            let at = |k: usize| -spread / 2.0 + spread * k as f32 / (n - 1).max(1) as f32;
            for k in 0..n {
                let a = at(k);
                let (x0, y0) = kit::polar(cx, cy, rr + 2.0, a);
                let (x1, y1) = kit::polar(cx, cy, rr + 5.0, a);
                p.line_segment(
                    [f.p(x0, y0), f.p(x1, y1)],
                    Stroke::new(f.s(1.2).max(0.7), c32(c.ink)),
                );
                let (lx, ly) = kit::polar(cx, cy, rr + 10.0, a);
                let col = if k == sel {
                    c.ink
                } else {
                    kit::mix(c.ink, 0x808080, 0.45)
                };
                pr.t(
                    &opt(k),
                    lx,
                    ly,
                    Align2::CENTER_CENTER,
                    osize,
                    col,
                    c.lab.style,
                    40.0,
                );
            }
            let u = (at(sel) + 135.0) / 270.0;
            kit::pointer(p, f, ks, cx, cy, rr, u);
        }
        ChK::HalfMoon => {
            let (cx, cy) = (c.x + c.w / 2.0, c.y + c.h * 0.78);
            let rr = c.w * 0.3;
            let deg = if sel == 0 { -22.0 } else { 22.0 };
            let pts: Vec<Pos2> = (0..=16)
                .map(|i| {
                    let a = -90.0 + 180.0 * i as f32 / 16.0 + deg;
                    let (x, y) = kit::polar(cx, cy, rr, a);
                    f.p(x, y)
                })
                .collect();
            let sh: Vec<Pos2> = pts.iter().map(|q| *q + vec2(-f.s(2.0), f.s(2.0))).collect();
            p.add(eframe::egui::Shape::convex_polygon(
                sh,
                Color32::from_black_alpha(110),
                Stroke::NONE,
            ));
            p.add(eframe::egui::Shape::convex_polygon(
                pts,
                Color32::from_rgb(236, 226, 200),
                Stroke::new(f.s(0.7).max(0.5), Color32::from_rgb(150, 136, 100)),
            ));
            for k in 0..n {
                let x = c.x + c.w * if k == 0 { 0.12 } else { 0.88 };
                let col = if k == sel {
                    c.ink
                } else {
                    kit::mix(c.ink, 0x000000, 0.45)
                };
                pr.t(
                    &opt(k),
                    x,
                    c.y + c.h * 0.12,
                    Align2::CENTER_CENTER,
                    osize,
                    col,
                    c.lab.style,
                    c.w * 0.5,
                );
            }
        }
        ChK::Art => {}
    }
    if let Some((lx, ly, anchor)) = label_at(&c.lab, c.x + c.w / 2.0, c.y + c.h / 2.0, c.h / 2.0) {
        pr.t(
            &label_text(&c.lab, kd),
            lx,
            ly,
            anchor,
            c.lab.size,
            c.lab.col,
            c.lab.style,
            c.w + 20.0,
        );
    }
    if hot {
        p.rect_stroke(
            f.rect(c.x, c.y, c.w, c.h).expand(1.5),
            3.0,
            Stroke::new(1.2, t().accent),
            eframe::egui::StrokeKind::Outside,
        );
    }
    let _ = c32a;
}
