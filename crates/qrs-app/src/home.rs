//! Home: the page the app opens on, the first page in the sidebar (port of rsplay/homeview.py). A Carry on card
//! with the last song you played (Enter to play it again) and your best runs of it, a row of recently played songs,
//! and a Settings strip summarising every section of the settings (click one to open it). Laid out in the Python
//! app's whole pixels, as the library (`px`, `snap`).

use std::collections::HashMap;

use chrono::{Datelike, Local, TimeZone};
use eframe::egui;
use egui::{Align2, Event, Key, Rect, Sense, Ui, Vec2, pos2, vec2};
use qrs_ui::icons::{self, Icon};
use qrs_ui::keys::{CapOn, keycap};
use qrs_ui::paint::{self, Fill};
use qrs_ui::text::{self, Style};
use qrs_ui::theme::t;
use qrs_ui::units::{pixels, px};
use qrs_ui::widgets::{ButtonStyle, button, button_width, interact};

use crate::library::{
    LibView, Req, View, caps, centerx, centery, draw_search, inflate, ipx, last_part, line, line_w,
    page_rects, scale, snap, trunc_px, txt,
};
use crate::state::AppState;

pub const SECTIONS: [(&str, &str); 5] = [
    ("audio", "Audio"),
    ("tone", "Guitar tone"),
    ("display", "Display"),
    ("practice", "Practice"),
    ("library", "Library"),
];

fn local(t: f64) -> Option<chrono::DateTime<Local>> {
    Local.timestamp_opt(t as i64, 0).single()
}

/// "played today, 20:12", "played yesterday, 20:12", "played Monday, 20:12", or the date further back.
pub fn played_when(t: f64) -> String {
    let (Some(lt), now) = (local(t), Local::now()) else {
        return String::new();
    };
    let days = (now.date_naive() - lt.date_naive()).num_days();
    let clock = lt.format("%H:%M");
    match days {
        ..=0 => format!("played today, {clock}"),
        1 => format!("played yesterday, {clock}"),
        2..=6 => format!("played {}, {clock}", lt.format("%A")),
        _ => {
            let year = if lt.year() != now.year() {
                format!(" {}", lt.year())
            } else {
                String::new()
            };
            format!("played {} {}{year}", lt.day(), lt.format("%b"))
        }
    }
}

/// When a run was played: "13 Oct 17:42" (with the year when asked, or when it isn't this year).
pub fn fmt_when(at: Option<f64>, year: bool) -> String {
    let Some(lt) = at.and_then(local) else {
        return "date not recorded".into();
    };
    let show_year = year || lt.year() != Local::now().year();
    format!(
        "{} {}{} {}",
        lt.day(),
        lt.format("%b"),
        if show_year {
            format!(" {},", lt.year())
        } else {
            String::new()
        },
        lt.format("%H:%M")
    )
}

#[derive(Default)]
pub struct HomeView {
    pub reqs: Vec<Req>,
}

impl HomeView {
    /// Rows by when they were last played, newest first.
    fn recent(st: &AppState, n: usize) -> Vec<usize> {
        let mut v: Vec<(f64, usize)> = (0..st.groups.len())
            .filter(|&g| st.listed(g))
            .filter_map(|g| st.userdata.last_played(&st.ids(g)).map(|t| (t, g)))
            .collect();
        v.sort_by(|a, b| b.0.total_cmp(&a.0));
        v.into_iter().take(n).map(|x| x.1).collect()
    }

    /// The cards under Carry on: recently played, then favourites, then a few picks from the library (the same ones
    /// all day).
    fn shelf(st: &AppState, recent: &[usize]) -> (Vec<usize>, String) {
        let mut out: Vec<usize> = recent.iter().skip(1).copied().collect();
        let mut title = "Recently played".to_string();
        // the favourites in the order of their ids, as the Python app's sorted(favorites)
        let mut row_of: HashMap<String, usize> = HashMap::new();
        for g in (0..st.groups.len()).filter(|&g| st.listed(g)) {
            for id in st.ids(g) {
                row_of.entry(id).or_insert(g);
            }
        }
        for sid in &st.userdata.favorites {
            let Some(&g) = row_of.get(sid) else { continue };
            if !out.contains(&g) && recent.first() != Some(&g) {
                out.push(g);
                title = if recent.len() > 1 {
                    "Recently played & favourites".into()
                } else {
                    "Your favourites".into()
                };
            }
        }
        let listed: Vec<usize> = (0..st.groups.len())
            .filter(|&g| st.listed(g) && st.song(g).playable())
            .collect();
        if out.len() < 6 && !listed.is_empty() {
            // a few picks, the same all day
            let day = Local::now().num_days_from_ce() as u64;
            let mut x = day.wrapping_mul(0x9E3779B97F4A7C15) | 1;
            let mut extra = Vec::new();
            for _ in 0..12 {
                x ^= x << 13;
                x ^= x >> 7;
                x ^= x << 17;
                let g = listed[(x % listed.len() as u64) as usize];
                if !out.contains(&g) && !extra.contains(&g) && recent.first() != Some(&g) {
                    extra.push(g);
                }
            }
            extra.truncate(6 - out.len());
            if !extra.is_empty() {
                title = if out.is_empty() {
                    "Something to try".into()
                } else {
                    format!("{title} · something to try")
                };
                out.extend(extra);
            }
        }
        (out, title)
    }

    pub fn handle(&mut self, lv: &mut LibView, st: &mut AppState, ev: &Event) -> bool {
        match ev {
            Event::Key {
                key,
                pressed: true,
                modifiers,
                ..
            } => {
                let recent = Self::recent(st, 1);
                match key {
                    Key::Enter | Key::Space if !recent.is_empty() => self.play(lv, st, recent[0]),
                    Key::L | Key::Tab => {
                        lv.go(st, View::All);
                        if *key == Key::L {
                            lv.skip_text = Some('l'); // the same key press also types "l": don't search for it
                        }
                    }
                    Key::Enter => lv.go(st, View::All),
                    Key::F if modifiers.command => lv.go(st, View::All),
                    Key::P if !recent.is_empty() => {
                        lv.go(st, View::All);
                        lv.open_picker(st, recent[0]);
                    }
                    _ => return false,
                }
                true
            }
            Event::MouseWheel { .. } => {
                let area = st.ctx.content_rect();
                lv.handle(st, area, ev)
            }
            _ => false,
        }
    }

    fn play(&mut self, lv: &mut LibView, st: &mut AppState, g: usize) {
        if let Some(a) = last_part(st, st.song(g)).cloned() {
            st.config.set("last_arrangement", a.name.to_lowercase());
            lv.view = View::All;
            self.reqs.push(Req::Play { g, part: a.key });
        }
    }

    pub fn ui(&mut self, ui: &mut Ui, lv: &mut LibView, st: &mut AppState, area: Rect) {
        let p = ui.painter().clone();
        let th = t();
        let (side, main) = page_rects(area);
        lv.draw_sidebar(ui, st, side);
        if lv.view != View::Home {
            return; // the sidebar went somewhere else
        }
        let s = scale();
        let hy = main.min.y + px(26.0);
        txt(
            &p,
            "Home",
            pos2(main.min.x, hy),
            28.0,
            th.text,
            800,
            Align2::LEFT_CENTER,
            None,
        );
        let bw = trunc_px((440.0 * s).min(ipx(main.width()) * 0.42));
        let boxr = Rect::from_min_size(pos2(main.max.x - bw, hy - px(26.0)), vec2(bw, px(52.0)));
        let total = lv.counts(st).all;
        draw_search(
            &p,
            boxr,
            "",
            &format!("Search {} songs", group_digits(total)),
            &["Ctrl", "F"],
        );
        if interact(ui, boxr, "home-search", Sense::click()).clicked() {
            lv.go(st, View::All);
        }
        let recent = Self::recent(st, 8);
        let strip = Rect::from_min_size(
            pos2(main.min.x, main.max.y - px(72.0)),
            vec2(main.width(), px(72.0)),
        );
        self.draw_settings_strip(ui, st, strip);
        let top = hy + px(26.0) + px(24.0);
        let room = strip.min.y - px(24.0) - top; // for the hero and the row of covers under it
        let hero_h = trunc_px((220.0 * s).max((300.0 * s).min(ipx(room) - 300.0 * s)));
        let hero = Rect::from_min_size(pos2(main.min.x, top), vec2(main.width(), hero_h));
        match recent.first() {
            Some(&g) => self.draw_carry_on(ui, lv, st, hero, g),
            None => self.draw_welcome(ui, lv, st, hero),
        }
        let low_y = hero.max.y + px(24.0);
        let low = Rect::from_min_size(
            pos2(main.min.x, low_y),
            vec2(main.width(), strip.min.y - px(24.0) - low_y),
        );
        let (shelf, title) = Self::shelf(st, &recent);
        self.draw_recent(ui, lv, st, low, &shelf, &title);
    }

    /// The last song you played: a big cover, title, part and tuning, your best and when you played it, Play and
    /// Pick a part; your best runs of that part in a screen at the right.
    fn draw_carry_on(&mut self, ui: &Ui, lv: &mut LibView, st: &mut AppState, r: Rect, g: usize) {
        let p = ui.painter().clone();
        let th = t();
        let ctx = st.ctx.clone();
        paint::raised(&p, r, Some(30.0), 8.0, Fill::Base, 0.0);
        let pad = px(24.0);
        let size = r.height() - 2.0 * pad;
        let art_r = Rect::from_min_size(r.min + vec2(pad, pad), Vec2::splat(size));
        let e = st.song(g).clone();
        if !crate::covers::draw(&p, &st.covers, &e, ipx(size - px(16.0)), art_r, 22.0, true) {
            icons::icon(
                &p,
                Icon::Note,
                pos2(centerx(art_r), centery(art_r)),
                44.0,
                th.caption,
            );
        }
        let Some(arr) = last_part(st, &e).cloned() else {
            return;
        };
        let ids = st.ids(g);
        let runs_w = trunc_px((300.0 * scale()).min(ipx(r.width()) * 0.28));
        let runs = Rect::from_min_size(pos2(r.max.x - pad - runs_w, r.min.y + pad), vec2(runs_w, size));
        self.draw_best_runs(&p, st, runs, &ids, &arr.name);
        let x = art_r.max.x + px(32.0);
        let tw = runs.min.x - px(32.0) - x;
        // the middle column, centred top to bottom
        let block =
            px(16.0) + px(10.0) + px(44.0) + px(10.0) + px(22.0) + px(14.0) + px(34.0) + px(22.0) + px(52.0);
        let mut y = r.min.y + pad.max(pixels(((ipx(r.height()) - ipx(block)) / 2.0).floor() as f32));
        caps(&p, "CARRY ON", pos2(x, y), 12.0, Align2::LEFT_TOP, None);
        y += px(16.0) + px(8.0);
        let tr = txt(
            &p,
            &e.display_title(),
            pos2(x, y),
            40.0,
            th.text,
            800,
            Align2::LEFT_TOP,
            Some(tw),
        );
        y = tr.max.y + px(4.0);
        txt(
            &p,
            &format!("{}  ·  {}  ·  {}", e.artist, arr.name, arr.tuning),
            pos2(x, y),
            16.0,
            th.text_2,
            600,
            Align2::LEFT_TOP,
            Some(tw),
        );
        y += px(22.0) + px(14.0);
        let mut chips: Vec<(&str, String, u16)> = Vec::new();
        if let Some((score, acc, _)) = st.userdata.best(&ids, Some(&arr.name)) {
            chips.push((
                "BEST",
                format!(
                    "{}  ·  {}%",
                    group_digits(score as usize),
                    (acc * 100.0).round() as i64
                ),
                800,
            ));
        }
        if let Some(t0) = st.userdata.last_played(&ids) {
            chips.push((
                "PLAYED",
                played_when(t0).trim_start_matches("played ").to_string(),
                700,
            ));
        }
        let mut cx = x;
        for (label, value, weight) in chips {
            let lw = text::width(&ctx, label, Style::caps(10.0, th.caption).tracked(0.1));
            let chip = Rect::from_min_size(
                pos2(cx, y),
                vec2(
                    lw + px(8.0) + text::width(&ctx, &value, Style::new(13.0, weight, th.text)) + px(24.0),
                    px(34.0),
                ),
            );
            if ipx(chip.max.x) > ipx(x + tw) {
                break;
            }
            paint::inset(&p, chip, Some(10.0), 2.0, Fill::Base);
            let lr = caps(
                &p,
                label,
                pos2(chip.min.x + px(12.0), centery(chip)),
                10.0,
                Align2::LEFT_CENTER,
                None,
            );
            txt(
                &p,
                &value,
                pos2(lr.max.x + px(8.0), centery(chip)),
                13.0,
                th.text,
                weight,
                Align2::LEFT_CENTER,
                None,
            );
            cx = chip.max.x + px(10.0);
        }
        y += px(34.0) + px(22.0);
        let label = format!("Play {}", arr.name);
        let pw = (button_width(&ctx, &label, "Enter", 16.0, true) + px(16.0)).min(tw);
        let play = Rect::from_min_size(pos2(x, y), vec2(pw, px(52.0)));
        let kw = button_width(&ctx, "Pick a part", "P", 15.0, false) + px(12.0);
        let pick = Rect::from_min_size(pos2(play.max.x + px(14.0), y), vec2(kw, px(52.0)));
        if button(
            ui,
            play,
            "home-play",
            &label,
            "Enter",
            ButtonStyle {
                primary: true,
                icon: Some(Icon::Play),
                size: 16.0,
                r: Some(16.0),
                ..Default::default()
            },
        )
        .clicked()
        {
            self.play(lv, st, g);
        }
        if ipx(pick.max.x) <= ipx(x + tw)
            && button(
                ui,
                pick,
                "home-pick",
                "Pick a part",
                "P",
                ButtonStyle {
                    size: 15.0,
                    r: Some(16.0),
                    ..Default::default()
                },
            )
            .clicked()
        {
            lv.go(st, View::All);
            lv.open_picker(st, g);
        }
    }

    /// Your best three runs of a part on a small screen: score, when, and a bar of its accuracy.
    fn draw_best_runs(&self, p: &egui::Painter, st: &AppState, r: Rect, ids: &[String], part: &str) {
        let th = t();
        paint::display(p, r, Some(20.0), 5.0);
        let (x, w) = (r.min.x + px(18.0), r.width() - px(36.0));
        caps(
            p,
            &format!("YOUR BEST RUNS  ·  {}", part.to_uppercase()),
            pos2(x, r.min.y + px(16.0)),
            11.0,
            Align2::LEFT_TOP,
            Some(w),
        );
        let top = st.userdata.top(ids, part);
        let mut y = r.min.y + px(16.0) + px(16.0) + px(14.0);
        if top.is_empty() {
            for line in text::wrap(
                &st.ctx,
                "None yet: play the whole part at full speed and difficulty to set one.",
                Style::new(13.0, 500, th.caption),
                w + pixels(0.01), // (whole pixels: a hair of room for f32, as blit_text's)
            ) {
                txt(
                    p,
                    &line,
                    pos2(x, y),
                    13.0,
                    th.caption,
                    500,
                    Align2::LEFT_TOP,
                    None,
                );
                y += px(19.0);
            }
            return;
        }
        for (i, run) in top.iter().take(3).enumerate() {
            if ipx(y + px(38.0)) > ipx(r.max.y - px(12.0)) {
                break;
            }
            let col = if i == 0 { th.accent_ink } else { th.text_2 };
            txt(
                p,
                &format!("{}   {}", i + 1, group_digits(run.score as usize)),
                pos2(x, y),
                14.0,
                col,
                800,
                Align2::LEFT_TOP,
                None,
            );
            txt(
                p,
                &fmt_when(run.at, false),
                pos2(x + w, y + px(2.0)),
                12.0,
                th.caption,
                500,
                Align2::RIGHT_TOP,
                None,
            );
            let bar = Rect::from_min_size(pos2(x, y + px(24.0)), vec2(w, px(8.0)));
            paint::inset(p, bar, None, 2.0, Fill::Base);
            if let Some(acc) = run.acc
                && acc > 0.0
            {
                let m = px(2.0).max(pixels(1.0));
                let fill = Rect::from_min_size(
                    bar.min + vec2(m, m),
                    vec2(
                        trunc_px(ipx(bar.width() - 2.0 * m) * acc).max(bar.height() - 2.0 * m),
                        bar.height() - 2.0 * m,
                    ),
                );
                if i == 0 {
                    paint::glow(p, fill, th.accent, 4.0, None, None);
                }
                paint::pill(p, fill, if i == 0 { th.accent } else { th.bar_line });
            }
            y += px(48.0);
        }
        txt(
            p,
            "bars show accuracy",
            pos2(x, r.max.y - px(14.0)),
            12.0,
            th.caption,
            500,
            Align2::LEFT_BOTTOM,
            None,
        );
    }

    fn draw_welcome(&mut self, ui: &Ui, lv: &mut LibView, st: &mut AppState, r: Rect) {
        let p = ui.painter().clone();
        let th = t();
        paint::raised(&p, r, Some(30.0), 8.0, Fill::Base, 0.0);
        let (x, y) = (r.min.x + px(40.0), r.min.y + px(36.0));
        let msg = if st.scanning() {
            "Reading your library…"
        } else {
            "Welcome to quickRS"
        };
        caps(&p, "START HERE", pos2(x, y), 12.0, Align2::LEFT_TOP, None);
        txt(
            &p,
            msg,
            pos2(x, y + px(22.0)),
            40.0,
            th.text,
            800,
            Align2::LEFT_TOP,
            None,
        );
        txt(
            &p,
            "Pick a song from your library to start. The last one you played will wait for you here.",
            pos2(x, y + px(78.0)),
            16.0,
            th.text_2,
            600,
            Align2::LEFT_TOP,
            Some(r.width() - px(80.0)),
        );
        let b = Rect::from_min_size(pos2(x, r.max.y - px(36.0) - px(52.0)), vec2(px(260.0), px(52.0)));
        if button(
            ui,
            b,
            "home-open-lib",
            "Open the library",
            "L",
            ButtonStyle {
                primary: true,
                size: 16.0,
                r: Some(16.0),
                ..Default::default()
            },
        )
        .clicked()
        {
            lv.go(st, View::All);
        }
    }

    /// One row of covers, each on a raised frame, with title, artist and your best under it.
    fn draw_recent(
        &mut self,
        ui: &Ui,
        lv: &mut LibView,
        st: &mut AppState,
        r: Rect,
        songs: &[usize],
        title: &str,
    ) {
        let p = ui.painter().clone();
        let th = t();
        let s = scale();
        let hdr = px(28.0);
        let text_h = px(10.0) + px(54.0);
        let gap = px(24.0);
        let rw = ipx(r.width());
        let cols: usize = if rw >= 900.0 * s {
            6
        } else if rw >= 700.0 * s {
            5
        } else {
            4
        };
        let cw = (rw - ipx(gap) * (cols - 1) as f64) / cols as f64; // pixels, not whole
        let size_px = cw.min(ipx(r.height() - hdr - px(14.0) - text_h)).trunc();
        if size_px < 70.0 * s {
            return;
        }
        let size = pixels(size_px as f32);
        let hcy = r.min.y + snap(hdr / 2.0);
        txt(
            &p,
            title,
            pos2(r.min.x, hcy),
            20.0,
            th.text,
            800,
            Align2::LEFT_CENTER,
            Some(r.width() * 0.6),
        );
        let kr = keycap(
            &p,
            "L",
            pos2(r.max.x, hcy),
            Align2::RIGHT_CENTER,
            11.0,
            CapOn::Material,
        );
        let link = txt(
            &p,
            "Open the library",
            pos2(kr.min.x - px(8.0), hcy),
            14.0,
            th.text_2,
            700,
            Align2::RIGHT_CENTER,
            None,
        );
        let hit = inflate(link.union(kr), px(10.0), px(10.0));
        if interact(ui, hit, "home-lib-link", Sense::click()).clicked() {
            lv.go(st, View::All);
        }
        let top = r.min.y + hdr + px(14.0);
        let mut open: Option<usize> = None;
        for (i, &g) in songs.iter().take(cols).enumerate() {
            let x = pixels((ipx(r.min.x) + i as f64 * (cw + ipx(gap))).trunc() as f32);
            let card = Rect::from_min_size(pos2(x, top), vec2(trunc_px(cw), size + text_h));
            let resp = interact(ui, card, ("home-card", g), Sense::click());
            let hot = resp.hovered();
            if hot {
                st.ctx.set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            let lift = if hot { px(2.0).max(pixels(1.0)) } else { 0.0 };
            let frame = Rect::from_min_size(pos2(x, top), Vec2::splat(size));
            paint::raised(&p, frame, Some(20.0), 3.0, Fill::Base, lift);
            let frame = frame.translate(vec2(0.0, -lift));
            let pad = px(6.0);
            let inner = frame.shrink(pad);
            let e = st.song(g).clone();
            match crate::covers::draw(&p, &st.covers, &e, ipx(size - 2.0 * pad), inner, 15.0, false) {
                true => {}
                false => {
                    paint::rrect(&p, inner, px(15.0), th.screen);
                    icons::icon(
                        &p,
                        Icon::Note,
                        pos2(centerx(inner), centery(inner)),
                        30.0,
                        th.caption,
                    );
                }
            }
            let ty = frame.max.y + lift + px(10.0);
            let tw = trunc_px(cw) - px(8.0);
            txt(
                &p,
                &e.display_title(),
                pos2(x + px(4.0), ty),
                14.0,
                th.text,
                800,
                Align2::LEFT_TOP,
                Some(tw),
            );
            txt(
                &p,
                &e.artist,
                pos2(x + px(4.0), ty + px(19.0)),
                12.0,
                th.caption,
                500,
                Align2::LEFT_TOP,
                Some(tw),
            );
            let best = st.userdata.best(&st.ids(g), None);
            let btxt = match best {
                Some((score, acc, _)) => format!(
                    "best {}  ·  {}%",
                    group_digits(score as usize),
                    (acc * 100.0).round() as i64
                ),
                None => "no score yet".into(),
            };
            txt(
                &p,
                &btxt,
                pos2(x + px(4.0), ty + px(36.0)),
                12.0,
                if best.is_some() { th.text_2 } else { th.caption },
                700,
                Align2::LEFT_TOP,
                Some(tw),
            );
            if resp.clicked() {
                open = Some(g);
            }
        }
        if let Some(g) = open {
            lv.go(st, View::All);
            lv.open_picker(st, g);
        }
    }

    fn summaries(&self, st: &AppState) -> Vec<String> {
        let cfg = &st.config;
        let audio = match &st.audio {
            Some(a) => {
                let s = a.engine.status();
                let inp = s
                    .input
                    .as_ref()
                    .map(|i| i.device_name.clone())
                    .unwrap_or_else(|| "no guitar input".into());
                let out = s
                    .output
                    .as_ref()
                    .map(|o| o.device_name.clone())
                    .unwrap_or_default();
                format!("{inp}  →  {out}")
            }
            None => "audio engine off".into(),
        };
        let tone = match &st.audio {
            Some(a) => {
                let label = match a.engine.tone() {
                    qrs_audio::tone::ToneSpec::AsIs => "As is".to_string(),
                    qrs_audio::tone::ToneSpec::Preset { preset } => preset.label().to_string(),
                    qrs_audio::tone::ToneSpec::Custom { .. } => "Custom".to_string(),
                };
                if a.engine.monitor() {
                    label
                } else {
                    format!("{label}  ·  not hearing guitar")
                }
            }
            None => "–".into(),
        };
        let display = format!(
            "Note tile: {}  ·  string colours {}",
            if cfg.str("note_view", "piano") == "guitar" {
                "guitar"
            } else {
                "piano"
            },
            if cfg.bool("string_colors", false) {
                "on"
            } else {
                "off"
            }
        );
        let practice = format!(
            "Level-up {}  ·  sync {:+.0} ms",
            if cfg.bool("auto_level", true) { "on" } else { "off" },
            cfg.f64("av_offset", 0.0) * 1000.0
        );
        let n = st.dirs.len();
        let library = format!(
            "{} songs  ·  {n} folder{}",
            group_digits(st.groups.len()),
            if n == 1 { "" } else { "s" }
        );
        vec![audio, tone, display, practice, library]
    }

    /// Every section of the settings in one raised strip, each with a one-line summary; click one to open it.
    fn draw_settings_strip(&mut self, ui: &Ui, st: &AppState, r: Rect) {
        let p = ui.painter().clone();
        let th = t();
        paint::raised(&p, r, Some(22.0), 6.0, Fill::Base, 0.0);
        let rcy = centery(r);
        let tr = txt(
            &p,
            "Settings",
            pos2(r.min.x + px(22.0), rcy),
            15.0,
            th.text,
            800,
            Align2::LEFT_CENTER,
            None,
        );
        keycap(
            &p,
            "F2",
            pos2(tr.max.x + px(8.0), rcy),
            Align2::LEFT_CENTER,
            11.0,
            CapOn::Material,
        );
        let x0 = r.min.x + px(142.0);
        let sums = self.summaries(st);
        // each cell's width in pixels, not whole: the cells start at int(x0 + i * w) and are int(w) wide
        let w = (ipx(r.max.x - px(10.0)) - ipx(x0)) / SECTIONS.len() as f64;
        for (i, (key, label)) in SECTIONS.iter().enumerate() {
            let cell = Rect::from_min_size(
                pos2(trunc_px(ipx(x0) + i as f64 * w), r.min.y + px(10.0)),
                vec2(trunc_px(w), r.height() - px(20.0)),
            );
            let resp = interact(ui, cell, ("home-set", *key), Sense::click());
            if resp.hovered() {
                paint::raised(&p, cell, Some(14.0), 3.0, Fill::Base, 0.0);
                st.ctx.set_cursor_icon(egui::CursorIcon::PointingHand);
            } else if i > 0 {
                line(
                    &p,
                    pos2(cell.min.x, cell.min.y + px(10.0)),
                    pos2(cell.min.x, cell.max.y - px(10.0)),
                    line_w(),
                    th.line,
                );
            }
            let title = if *key == "tone" {
                "TONE".to_string()
            } else {
                label.to_uppercase()
            };
            let ccy = centery(cell);
            caps(
                &p,
                &title,
                pos2(cell.min.x + px(14.0), ccy - px(1.0)),
                10.0,
                Align2::LEFT_BOTTOM,
                Some(cell.width() - px(24.0)),
            );
            txt(
                &p,
                &sums[i],
                pos2(cell.min.x + px(14.0), ccy + px(1.0)),
                13.0,
                th.text_2,
                700,
                Align2::LEFT_TOP,
                Some(cell.width() - px(24.0)),
            );
            if resp.clicked() {
                self.reqs.push(Req::Settings(key));
            }
        }
    }
}

/// 1588 -> "1,588"
pub fn group_digits(n: usize) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, ch) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(ch);
    }
    out
}
