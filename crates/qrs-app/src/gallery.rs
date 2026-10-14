//! The component gallery (P5.7): every part of the kit in its states, on one page, in the current theme. A debug
//! screen (`quickrs --gallery`) and the first thing screenshot mode captures.

use eframe::egui;
use egui::{Align2, Pos2, Rect, Sense, Ui, Vec2, pos2, vec2};
use qrs_ui::Mat;
use qrs_ui::icons::{self, Icon};
use qrs_ui::keys::{self, CapOn, Gem};
use qrs_ui::knobs::{self, KnobStyle, Scale};
use qrs_ui::paint;
use qrs_ui::text::{self, Style};
use qrs_ui::theme::{self, ThemeName, t};
use qrs_ui::widgets::{self, ButtonStyle, RoundKind, Segmented};

pub struct Gallery {
    part: usize,
    tone: usize,
    sw: [bool; 3],
    vol: f32,
    eq: f32,
    knob_u: [f32; 4],
}

impl Default for Gallery {
    fn default() -> Self {
        Gallery {
            part: 0,
            tone: 1,
            sw: [true, false, true],
            vol: 0.62,
            eq: 0.35,
            knob_u: [0.7, 0.45, 0.55, 0.3],
        }
    }
}

fn heading(p: &egui::Painter, label: &str, at: Pos2) {
    text::caps(p, label, at, 12.0, t().caption, Align2::LEFT_TOP, None);
}

impl Gallery {
    pub fn ui(&mut self, ui: &mut Ui, area: Rect) {
        let p = ui.painter().clone();
        let th = t();
        let o = area.min;
        let at = |x: f32, y: f32| pos2(o.x + x, o.y + y);
        let r = |x: f32, y: f32, w: f32, h: f32| Rect::from_min_size(at(x, y), vec2(w, h));

        // title and the theme / knob pickers
        text::draw(
            &p,
            "Component gallery",
            at(28.0, 22.0),
            Align2::LEFT_TOP,
            Style::new(30.0, 800, th.text),
            None,
        );
        text::draw(
            &p,
            "qrs-ui: the neumorphic kit (assets/design/REDESIGN.md); every component in its states",
            at(30.0, 62.0),
            Align2::LEFT_TOP,
            Style::new(14.0, 600, th.text_2),
            None,
        );
        let names: Vec<&str> = ThemeName::ALL.iter().map(|n| n.label()).collect();
        let cur = ThemeName::ALL.iter().position(|n| *n == theme::theme());
        let (_, hit) = widgets::segmented(
            ui,
            r(700.0, 22.0, 380.0, 50.0),
            "g-theme",
            &Segmented::new(&names, cur),
        );
        if let Some(i) = hit {
            theme::set_theme(ThemeName::ALL[i]);
        }
        let ks: Vec<&str> = KnobStyle::ALL.iter().map(|k| k.key()).collect();
        let kcur = KnobStyle::ALL.iter().position(|k| *k == knobs::style());
        let mut seg = Segmented::new(&ks, kcur);
        seg.size = 13.0;
        let (_, hit) = widgets::segmented(ui, r(1096.0, 22.0, 280.0, 50.0), "g-knobs", &seg);
        if let Some(i) = hit {
            knobs::set_style(KnobStyle::ALL[i]);
        }

        // -- surfaces --------------------------------------------------------------------------------------
        heading(&p, "SURFACES", at(28.0, 96.0));
        paint::card(&p, r(28.0, 120.0, 150.0, 96.0), 22.0, false);
        text::draw(
            &p,
            "Card",
            at(48.0, 140.0),
            Align2::LEFT_TOP,
            Style::new(15.0, 700, th.text),
            None,
        );
        paint::card(&p, r(196.0, 120.0, 150.0, 96.0), 22.0, true);
        text::draw(
            &p,
            "Lifted",
            at(216.0, 140.0),
            Align2::LEFT_TOP,
            Style::new(15.0, 700, th.text),
            None,
        );
        paint::well(&p, r(28.0, 236.0, 150.0, 96.0), 22.0);
        text::draw(
            &p,
            "Well",
            at(48.0, 256.0),
            Align2::LEFT_TOP,
            Style::new(15.0, 700, th.text_2),
            None,
        );
        paint::display(&p, r(196.0, 236.0, 150.0, 96.0), Some(18.0), 6.0);
        text::draw(
            &p,
            "Screen",
            at(216.0, 256.0),
            Align2::LEFT_TOP,
            Style::new(15.0, 700, th.text_2),
            None,
        );
        text::draw(
            &p,
            "0:42",
            at(330.0, 316.0),
            Align2::RIGHT_BOTTOM,
            Style::new(26.0, 800, th.accent_ink),
            None,
        );
        paint::floating(&p, r(28.0, 352.0, 318.0, 84.0), Some(26.0));
        text::draw(
            &p,
            "Floating panel",
            at(50.0, 372.0),
            Align2::LEFT_TOP,
            Style::new(15.0, 700, th.text),
            None,
        );
        text::draw(
            &p,
            "settings, results, dialogs",
            at(50.0, 396.0),
            Align2::LEFT_TOP,
            Style::new(13.0, 600, th.caption),
            None,
        );

        // -- keys ----------------------------------------------------------------------------------------------
        heading(&p, "KEYS", at(380.0, 96.0));
        let b = |ui: &Ui, x: f32, y: f32, w: f32, label: &str, key: &str, st: ButtonStyle, id: &str| {
            widgets::button(ui, r(x, y, w, 44.0), id, label, key, st)
        };
        b(
            ui,
            380.0,
            120.0,
            120.0,
            "Library",
            "Esc",
            ButtonStyle::default(),
            "k1",
        );
        b(
            ui,
            512.0,
            120.0,
            140.0,
            "Play",
            "Enter",
            ButtonStyle::primary(),
            "k2",
        );
        b(
            ui,
            664.0,
            120.0,
            130.0,
            "Loop",
            "L",
            ButtonStyle::active(true),
            "k3",
        );
        b(
            ui,
            380.0,
            176.0,
            120.0,
            "Lead",
            "",
            ButtonStyle {
                accent: true,
                ..Default::default()
            },
            "k4",
        );
        b(
            ui,
            512.0,
            176.0,
            140.0,
            "Delete tone",
            "",
            ButtonStyle {
                danger: true,
                ..Default::default()
            },
            "k5",
        );
        b(ui, 664.0, 176.0, 130.0, "100%", "", ButtonStyle::value(), "k6");
        b(
            ui,
            380.0,
            232.0,
            120.0,
            "Disabled",
            "",
            ButtonStyle {
                enabled: false,
                ..Default::default()
            },
            "k7",
        );
        b(
            ui,
            512.0,
            232.0,
            140.0,
            "Restart",
            "R",
            ButtonStyle::default().icon(Icon::Restart),
            "k8",
        );
        b(
            ui,
            664.0,
            232.0,
            130.0,
            "Stop",
            "",
            ButtonStyle::default().icon(Icon::Stop),
            "k9",
        );
        widgets::button(
            ui,
            r(380.0, 288.0, 414.0, 44.0),
            "k10",
            "Loop this section",
            "L",
            ButtonStyle::default().split(),
        );
        b(
            ui,
            380.0,
            344.0,
            120.0,
            "Play",
            "P",
            ButtonStyle {
                mat: Some(Mat::Tortoise),
                accent: true,
                ..Default::default()
            },
            "k11",
        );
        b(
            ui,
            512.0,
            344.0,
            140.0,
            "Synth",
            "",
            ButtonStyle {
                mat: Some(Mat::Abalone),
                seed: 3,
                ..Default::default()
            }
            .icon(Icon::Synth),
            "k12",
        );
        b(
            ui,
            664.0,
            344.0,
            130.0,
            "Part",
            "",
            ButtonStyle {
                mat: Some(Mat::Aluminium),
                seed: 1,
                ..Default::default()
            },
            "k13",
        );
        // key caps on each surface
        let mut x = 380.0;
        for (label, on) in [
            ("Space", CapOn::Material),
            ("Ctrl+P", CapOn::Material),
            ("F2", CapOn::Material),
        ] {
            let rr = keys::keycap(&p, label, at(x, 410.0), Align2::LEFT_CENTER, 11.0, on);
            x = rr.max.x - o.x + 10.0;
        }

        // -- round keys ------------------------------------------------------------------------------------
        heading(&p, "ROUND KEYS", at(830.0, 96.0));
        widgets::round_button(
            ui,
            r(830.0, 120.0, 56.0, 56.0),
            "rb1",
            Icon::Back,
            22.0,
            RoundKind::Ivory,
            false,
        );
        widgets::round_button(
            ui,
            r(898.0, 114.0, 68.0, 68.0),
            "rb2",
            Icon::Play,
            26.0,
            RoundKind::Gem(Gem::Emerald),
            false,
        );
        widgets::round_button(
            ui,
            r(978.0, 114.0, 68.0, 68.0),
            "rb3",
            Icon::Pause,
            26.0,
            RoundKind::Gem(Gem::Ruby),
            false,
        );
        widgets::round_button(
            ui,
            r(1058.0, 120.0, 56.0, 56.0),
            "rb4",
            Icon::Fwd,
            22.0,
            RoundKind::Ivory,
            false,
        );
        widgets::round_button(
            ui,
            r(1126.0, 120.0, 56.0, 56.0),
            "rb5",
            Icon::Lock,
            20.0,
            RoundKind::Mat(Mat::Tortoise, 5, None),
            false,
        );
        widgets::round_button(
            ui,
            r(1194.0, 132.0, 30.0, 30.0),
            "rb6",
            Icon::XBold,
            12.0,
            RoundKind::Gem(Gem::Ruby),
            false,
        );
        widgets::round_button(
            ui,
            r(1236.0, 120.0, 56.0, 56.0),
            "rb7",
            Icon::ArrowLeft,
            22.0,
            RoundKind::Ivory,
            false,
        );

        // -- controls ------------------------------------------------------------------------------------------
        heading(&p, "CONTROLS", at(830.0, 200.0));
        let parts = ["Lead", "Rhythm", "Bass"];
        let keys_ = ["1", "2", "3"];
        let mut seg = Segmented::new(&parts, Some(self.part));
        seg.keys = Some(&keys_);
        let (_, hit) = widgets::segmented(ui, r(830.0, 224.0, 400.0, 52.0), "g-part", &seg);
        if let Some(i) = hit {
            self.part = i;
        }
        let tones = ["As is", "Clean", "Crunch", "Lead"];
        let mut seg = Segmented::new(&tones, Some(self.tone));
        seg.size = 13.0;
        seg.mat = Some(Mat::Tortoise);
        let (_, hit) = widgets::segmented(ui, r(830.0, 288.0, 400.0, 46.0), "g-tone", &seg);
        if let Some(i) = hit {
            self.tone = i;
        }
        for (i, (sz, x)) in [("lg", 1250.0), ("md", 1250.0), ("sm", 1250.0)]
            .iter()
            .enumerate()
        {
            let s = widgets::switch_size(sz);
            let y = 224.0 + i as f32 * 40.0;
            if widgets::switch(ui, r(*x, y, s.x, s.y), ("g-sw", i), self.sw[i]).clicked() {
                self.sw[i] = !self.sw[i];
            }
        }
        // sliders: drag along the groove
        let g1 = r(830.0, 362.0, 250.0, 12.0);
        let resp = widgets::interact(ui, g1.expand(12.0), "g-sl1", Sense::click_and_drag());
        if let Some(pos) = resp.interact_pointer_pos()
            && resp.is_pointer_button_down_on()
        {
            self.vol = ((pos.x - g1.min.x) / g1.width()).clamp(0.0, 1.0);
        }
        widgets::slider(&p, g1, self.vol, None, 26.0, resp.hovered(), None);
        text::draw(
            &p,
            &format!("{:.0}%", self.vol * 100.0),
            at(1100.0, 368.0),
            Align2::LEFT_CENTER,
            Style::new(15.0, 800, th.accent_ink),
            None,
        );
        let g2 = r(830.0, 404.0, 250.0, 12.0);
        let resp = widgets::interact(ui, g2.expand(12.0), "g-sl2", Sense::click_and_drag());
        if let Some(pos) = resp.interact_pointer_pos()
            && resp.is_pointer_button_down_on()
        {
            self.eq = ((pos.x - g2.min.x) / g2.width()).clamp(0.0, 1.0);
        }
        widgets::slider(&p, g2, self.eq, Some(0.5), 26.0, resp.hovered(), None);
        text::draw(
            &p,
            &format!("{:+.1} dB", (self.eq - 0.5) * 24.0),
            at(1100.0, 410.0),
            Align2::LEFT_CENTER,
            Style::new(15.0, 800, th.accent_ink),
            None,
        );

        // -- knobs ---------------------------------------------------------------------------------------------
        heading(&p, "KNOBS", at(28.0, 460.0));
        for (i, st) in KnobStyle::ALL.iter().enumerate() {
            let c = at(80.0 + i as f32 * 118.0, 540.0);
            let rect = Rect::from_center_size(c, Vec2::splat(64.0));
            let resp = widgets::interact(ui, rect, ("g-knob", i), Sense::click_and_drag());
            if resp.dragged() {
                self.knob_u[i] = (self.knob_u[i] - resp.drag_delta().y / 200.0).clamp(0.0, 1.0);
            }
            knobs::knob(&p, c, 64.0, self.knob_u[i], Scale::default(), Some(*st));
            text::draw(
                &p,
                st.label(),
                pos2(c.x, c.y + 56.0),
                Align2::CENTER_TOP,
                Style::new(12.0, 700, th.caption),
                None,
            );
        }
        knobs::knob(
            &p,
            at(560.0, 560.0),
            120.0,
            0.62,
            Scale {
                home: Some(0.5),
                ..Default::default()
            },
            None,
        );
        knobs::knob(
            &p,
            at(680.0, 520.0),
            36.0,
            0.3,
            Scale {
                zero: Some(0.5),
                ..Default::default()
            },
            None,
        );
        text::draw(
            &p,
            "EQ",
            at(680.0, 556.0),
            Align2::CENTER_TOP,
            Style::caps(11.0, th.caption),
            None,
        );
        knobs::knob(
            &p,
            at(680.0, 610.0),
            36.0,
            0.8,
            Scale::default(),
            Some(KnobStyle::Vintage),
        );

        // -- gauges --------------------------------------------------------------------------------------------
        heading(&p, "GAUGES", at(760.0, 460.0));
        paint::display(&p, r(760.0, 484.0, 600.0, 170.0), Some(18.0), 6.0);
        knobs::ring_gauge(&p, at(840.0, 569.0), 46.0, 9.0, 0.72, None);
        text::draw(
            &p,
            "72%",
            at(840.0, 569.0),
            Align2::CENTER_CENTER,
            Style::new(22.0, 800, th.text),
            None,
        );
        knobs::arc_gauge(&p, r(920.0, 494.0, 260.0, 100.0), Some(18.0), th.late);
        text::draw(
            &p,
            "+18¢ · tune down",
            at(1050.0, 612.0),
            Align2::CENTER_TOP,
            Style::new(14.0, 700, th.late),
            None,
        );
        knobs::led_meter(&p, 0.3, r(1200.0, 520.0, 140.0, 10.0), 12);
        knobs::meter(&p, 0.12, r(1200.0, 550.0, 140.0, 8.0));
        for (i, c) in [th.accent, th.hit, th.miss].iter().enumerate() {
            paint::led(&p, at(1210.0 + i as f32 * 24.0, 590.0), 9.0, *c);
        }

        // -- icons -----------------------------------------------------------------------------------------------
        heading(&p, "ICONS", at(28.0, 676.0));
        let all = [
            Icon::Play,
            Icon::Pause,
            Icon::Stop,
            Icon::Back,
            Icon::Fwd,
            Icon::Down,
            Icon::Up,
            Icon::Right,
            Icon::Left,
            Icon::Search,
            Icon::Music,
            Icon::X,
            Icon::XBold,
            Icon::Check,
            Icon::Plus,
            Icon::Minus,
            Icon::Grip,
            Icon::Star,
            Icon::StarOff,
            Icon::Lock,
            Icon::Note,
            Icon::Home,
            Icon::Sliders,
            Icon::Grid,
            Icon::Restart,
            Icon::Expand,
            Icon::ArrowLeft,
            Icon::Synth,
            Icon::Track,
            Icon::Gear,
        ];
        for (i, ic) in all.iter().enumerate() {
            let c = at(44.0 + i as f32 * 40.0, 718.0);
            let col = if *ic == Icon::Star { th.accent } else { th.text_2 };
            icons::icon(&p, *ic, c, 22.0, col);
        }

        // -- text and toast --------------------------------------------------------------------------------------
        text::draw(
            &p,
            "Song complete",
            at(28.0, 760.0),
            Align2::LEFT_TOP,
            Style::new(28.0, 800, th.text),
            None,
        );
        text::draw(
            &p,
            "Sweet Child O' Mine · Lead · E Standard",
            at(250.0, 770.0),
            Align2::LEFT_TOP,
            Style::new(16.0, 600, th.text_2),
            None,
        );
        text::caps(
            &p,
            "NOTE · NEXT IN 0.4S",
            at(620.0, 772.0),
            12.0,
            th.caption,
            Align2::LEFT_TOP,
            None,
        );
        widgets::toast(&p, "Loop on: ", at(1050.0, 780.0), "bars 12-16");
    }
}
