//! Home: the page the app opens on, the first page in the sidebar (port of rsplay/homeview.py). A Carry on card
//! with the last song you played (Enter to play it again), your best runs of it and its last review (a click opens
//! it), a row of recently played songs, and a Settings strip summarising every section of the settings (click one to
//! open it). Laid out in the Python app's whole pixels, as the library (`px`, `snap`).

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

use crate::library::levels;
use crate::library::{
    LibView, RANK_W, Req, View, caps, centerx, centery, draw_rank, draw_search, inflate, ipx, last_part,
    line, line_w, page_rects, pick_random, random_seed, rank_tip, scale, snap, song_rank, trunc_px, txt,
};
use crate::player::history::{self, Gist};
use crate::state::AppState;

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

/// What Home shows from the library: the last 8 played and the shelf under Carry on, with its title.
#[derive(Clone)]
struct Shelves {
    recent: Vec<usize>,
    shelf: Vec<usize>,
    title: String,
    /// Easy songs to start with (P37.6b, D137): level up to `levels::STARTER_MAX`, the unplayed and the
    /// not-yet-good first, then the easiest.
    starters: Vec<usize>,
}

/// Where songs come from, in plain words (P37.6b: review 1 found psarc / feedpak unexplained), with the folders
/// they go in.
pub fn songs_note(st: &AppState) -> String {
    let dirs: Vec<String> = st.dirs.iter().map(|d| d.display().to_string()).collect();
    let put = if dirs.is_empty() {
        "add a folder of them in Settings › Library".to_string()
    } else {
        format!("put them in {}", dirs.join(" or "))
    };
    format!(
        "Songs are RS 2014 song files (.psarc: the game's own songs and the ones you bought for it, or \
         custom charts) or .feedpak files (an open format for charts and their audio). quickRS doesn't come with \
         songs: {put}, or drop a song file on the window."
    )
}

/// A part's newest saved run, for the Carry on card.
#[derive(Clone)]
struct LastReview {
    at: f64,
    acc: f64,
    gist: Option<Gist>,
}

#[derive(Default)]
pub struct HomeView {
    pub reqs: Vec<Req>,
    /// `Shelves` as of (the library's and the user data's versions, the day): working them out goes through every
    /// row (2-5 ms with ~3000), too much for every frame.
    shelves: Option<((u64, i32), Shelves)>,
    /// The carry-on part's last review as of (its key, reviews.json's time).
    review: Option<(String, Option<std::time::SystemTime>, Option<LastReview>)>,
    /// The shelf's tab picked by hand: true for Good to start with (None: that one while little has been played).
    starters_tab: Option<bool>,
}

impl HomeView {
    fn shelves(&mut self, st: &AppState) -> Shelves {
        let key = (
            st.version.wrapping_mul(1_000_003) ^ st.userdata.version,
            Local::now().num_days_from_ce(),
        );
        if let Some((k, s)) = &self.shelves
            && *k == key
        {
            return s.clone();
        }
        let recent = Self::recent(st, 8);
        let (shelf, title) = Self::shelf(st, &recent);
        let starters = Self::starters(st, 12);
        let s = Shelves {
            recent,
            shelf,
            title,
            starters,
        };
        self.shelves = Some((key, s.clone()));
        s
    }

    /// Rows by when they were last played, newest first.
    fn recent(st: &AppState, n: usize) -> Vec<usize> {
        let mut v: Vec<(f64, usize)> = (0..st.groups.len())
            .filter(|&g| st.listed(g))
            .filter_map(|g| st.userdata.last_played(&st.ids(g)).map(|t| (t, g)))
            .collect();
        v.sort_by(|a, b| b.0.total_cmp(&a.0));
        v.into_iter().take(n).map(|x| x.1).collect()
    }

    /// Songs good to start with (D137): the easy ones (level 1-3 for your instrument), those not played yet or
    /// not yet played to 80% first, then by level, the same order all day.
    fn starters(st: &AppState, n: usize) -> Vec<usize> {
        let bass = st.instrument().is_bass();
        let day = Local::now().num_days_from_ce() as u64;
        let mut v: Vec<(bool, u8, u64, usize)> = (0..st.groups.len())
            .filter(|&g| st.listed(g) && st.song(g).playable())
            // (a part for your instrument: the bass exercises aren't a guitarist's first songs)
            .filter(|&g| {
                levels::part_letters(st.song(g))
                    .iter()
                    .any(|c| (*c == 'B') == bass)
            })
            .filter_map(|g| {
                let lv = levels::song_level(st.song(g), bass)?;
                (lv <= levels::STARTER_MAX).then(|| {
                    let done = st
                        .userdata
                        .best_full_speed_acc(&st.ids(g))
                        .is_some_and(|a| a >= 0.8);
                    let mix = (g as u64 ^ day).wrapping_mul(0x9E3779B97F4A7C15);
                    (done, lv, mix, g)
                })
            })
            .collect();
        v.sort();
        v.into_iter().take(n).map(|x| x.3).collect()
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
                let recent = self.shelves(st).recent;
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
                    Key::A if !modifiers.command => self.reqs.push(Req::Room),
                    Key::P if !recent.is_empty() => lv.open_picker(st, recent[0]),
                    Key::R if modifiers.command => self.random(lv, st),
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

    /// Home's Random (P37.10): a random song of the whole library (any that plays), not the one played last,
    /// opened as a card is (the part picker over Home).
    fn random(&mut self, lv: &mut LibView, st: &mut AppState) {
        let cands: Vec<usize> = (0..st.groups.len())
            .filter(|&g| st.listed(g) && st.song(g).playable())
            .collect();
        let last = self.shelves(st).recent.first().copied();
        if let Some(g) = pick_random(&cands, &last.into_iter().collect::<Vec<_>>(), random_seed()) {
            lv.open_picker(st, g);
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
            &format!("Search {}", songs(total)),
            &["Ctrl", "F"],
            (false, false),
        );
        if interact(ui, boxr, "home-search", Sense::click()).clicked() {
            lv.go(st, View::All);
        }
        let Shelves {
            recent,
            shelf,
            title,
            starters,
        } = self.shelves(st);
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
        // the shelf: what you played (and favourites), or easy songs to start with (P37.6b, D137): the easy ones
        // first while little has been played
        let start = !starters.is_empty() && self.starters_tab.unwrap_or(recent.len() < 3);
        let tabs = if starters.is_empty() {
            vec![(title.as_str(), false)]
        } else {
            vec![(title.as_str(), false), ("Good to start with", true)]
        };
        // after an update (P37.15, D148): the card asking whether to run the setup again, in the shelf's place
        if st.whats_new {
            self.draw_whats_new(ui, st, low);
            return;
        }
        let songs = if start { &starters } else { &shelf };
        if let Some(pick) = self.draw_recent(ui, lv, st, low, songs, &tabs, start) {
            self.starters_tab = Some(pick);
        }
    }

    /// After an update (`whatsnew`): this version is here, what's new, and Run setup / Not now / Don't ask again.
    fn draw_whats_new(&mut self, ui: &Ui, st: &mut AppState, r: Rect) {
        use crate::whatsnew;
        let p = ui.painter().clone();
        let th = t();
        let ctx = st.ctx.clone();
        paint::card(&p, r, 24.0, false);
        let pad = px(28.0);
        let (x, y) = (r.min.x + pad, r.min.y + px(24.0));
        let gap = px(36.0);
        let lw = ((r.width() - 2.0 * pad - gap) * 0.48).max(px(300.0));
        caps(&p, "NEW VERSION", pos2(x, y), 12.0, Align2::LEFT_TOP, None);
        let head = format!("quickRS {} is here", crate::update::Version::current());
        let hr = text::draw(
            &p,
            &head,
            pos2(x, y + px(22.0)),
            Align2::LEFT_TOP,
            Style::new(26.0, 800, th.text),
            Some(lw),
        );
        let words = "A lot has changed: a new setup with a timing tool, bass mode, the amp room and more. Run the                      setup again to go through it? Your songs, scores and settings stay as they are.";
        let wst = Style::new(14.0, 600, th.text_2);
        let mut ly = hr.max.y + px(10.0);
        for l in text::wrap(&ctx, words, wst, lw).iter().take(4) {
            ly = text::draw(&p, l, pos2(x, ly), Align2::LEFT_TOP, wst, None).max.y + px(2.0);
        }
        // the keys: Run setup (the main one), Not now, Don't ask again
        let bh = px(44.0);
        let by = (r.max.y - px(22.0) - bh).max(ly + px(12.0));
        let mut bx = x;
        for (label, what, main) in [
            ("Run setup", "setup", true),
            ("Not now", "later", false),
            ("Don't ask again", "never", false),
        ] {
            let bw = button_width(&ctx, label, "", 14.0, false);
            let br = Rect::from_min_size(pos2(bx, by), vec2(bw, bh));
            if button(
                ui,
                br,
                ("home-whatsnew", what),
                label,
                "",
                ButtonStyle {
                    size: 14.0,
                    r: Some(14.0),
                    primary: main,
                    plain: what == "never",
                    ..Default::default()
                },
            )
            .clicked()
            {
                whatsnew::answer(&mut st.config, what);
                st.whats_new = false;
                st.save_config();
                if what == "setup" {
                    self.reqs.push(Req::Setup);
                }
            }
            bx = br.max.x + px(12.0);
        }
        // what's new, at the right
        let x2 = x + lw + gap;
        let w2 = r.max.x - pad - x2;
        if w2 < px(200.0) {
            return;
        }
        caps(&p, "WHAT'S NEW", pos2(x2, y), 12.0, Align2::LEFT_TOP, None);
        let ist = Style::new(14.0, 600, th.text);
        let mut iy = y + px(26.0);
        for item in whatsnew::WHATS_NEW {
            let lines = text::wrap(&ctx, item, ist, w2 - px(18.0));
            if iy + lines.len() as f32 * px(20.0) > r.max.y - px(14.0) {
                break;
            }
            p.circle_filled(pos2(x2 + px(4.0), iy + px(10.0)), px(3.0), th.accent);
            for l in &lines {
                iy = text::draw(&p, l, pos2(x2 + px(18.0), iy), Align2::LEFT_TOP, ist, None)
                    .max
                    .y;
            }
            iy += px(8.0);
        }
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
        if self.draw_best_runs(ui, st, runs, &ids, &arr.name) {
            // the last review, as E on that song
            st.config.set("last_arrangement", arr.name.to_lowercase());
            lv.view = View::All;
            self.reqs.push(Req::Review {
                g,
                part: arr.key.clone(),
            });
        }
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

    /// The newest saved run of a song's part (reviews.json, P29.3): when, its accuracy, its review's gist. Read again
    /// only when the song, the part or the file changes.
    fn last_review(&mut self, st: &AppState, ids: &[String], part: &str) -> Option<LastReview> {
        let path = st.data.root.join("reviews.json");
        let key = format!("{}|{part}", ids.join(","));
        let mtime = std::fs::metadata(&path).and_then(|m| m.modified()).ok();
        if let Some((k, m, r)) = &self.review
            && *k == key
            && *m == mtime
        {
            return r.clone();
        }
        let r = mtime.and_then(|_| {
            ids.iter()
                .filter_map(|id| history::load(&path, &format!("{id}|{part}")).into_iter().next())
                .max_by(|a, b| a.at.total_cmp(&b.at))
                .map(|run| LastReview {
                    at: run.at,
                    acc: run.acc,
                    gist: run.gist,
                })
        });
        self.review = Some((key, mtime, r.clone()));
        r
    }

    /// The part's screen at the right: your best three runs at the top (score, when, a bar of its accuracy), and
    /// from about the middle down the last review in a few lines (P34.6), a click opening it. True when clicked.
    fn draw_best_runs(&mut self, ui: &Ui, st: &AppState, r: Rect, ids: &[String], part: &str) -> bool {
        let p = ui.painter().clone();
        let th = t();
        paint::display(&p, r, Some(20.0), 5.0);
        let (x, w) = (r.min.x + px(18.0), r.width() - px(36.0));
        let wrap_w = w + pixels(0.01); // (whole pixels: a hair of room for f32, as blit_text's)
        caps(
            &p,
            &format!("YOUR BEST RUNS  ·  {}", part.to_uppercase()),
            pos2(x, r.min.y + px(16.0)),
            11.0,
            Align2::LEFT_TOP,
            Some(w),
        );
        // the last review's lines (text, size, weight, colour), wrapped
        let last = self.last_review(st, ids, part);
        let mut rows: Vec<(String, f32, u16, egui::Color32)> = Vec::new();
        match &last {
            Some(lr) => {
                for (i, l) in history::gist_lines(lr.acc, lr.gist.as_ref()).iter().enumerate() {
                    let (size, weight, col) = match i {
                        0 => (14.0, 800, th.text),
                        1 => (13.0, 600, th.text_2),
                        _ => (13.0, 500, th.caption),
                    };
                    for w in text::wrap(&st.ctx, l, Style::new(size, weight, col), wrap_w) {
                        rows.push((w, size, weight, col));
                    }
                }
            }
            None => {
                for w in text::wrap(
                    &st.ctx,
                    "Finish a run and press Review (E) to see where to practise.",
                    Style::new(13.0, 500, th.caption),
                    wrap_w,
                ) {
                    rows.push((w, 13.0, 500, th.caption));
                }
            }
        }
        let row_h = |size: f32| px(if size >= 14.0 { 20.0 } else { 18.0 });
        let head_h = px(16.0) + px(8.0);
        let bottom = r.max.y - px(14.0);
        let need = head_h + rows.iter().map(|x| row_h(x.1)).sum::<f32>();
        let rev_y = snap((bottom - need).min(r.min.y + r.height() * 0.55)).max(r.min.y + px(96.0));
        // the best runs, compact, above it
        let top = st.userdata.top(ids, part);
        let mut y = r.min.y + px(16.0) + px(16.0) + px(10.0);
        let runs_end = rev_y - px(10.0);
        if top.is_empty() {
            for line in text::wrap(
                &st.ctx,
                "None yet: play the whole part at full speed and difficulty to set one.",
                Style::new(13.0, 500, th.caption),
                wrap_w,
            ) {
                if ipx(y + px(18.0)) > ipx(runs_end) {
                    break;
                }
                txt(
                    &p,
                    &line,
                    pos2(x, y),
                    13.0,
                    th.caption,
                    500,
                    Align2::LEFT_TOP,
                    None,
                );
                y += px(18.0);
            }
        }
        for (i, run) in top.iter().take(3).enumerate() {
            if ipx(y + px(24.0)) > ipx(runs_end) {
                break;
            }
            let col = if i == 0 { th.accent_ink } else { th.text_2 };
            txt(
                &p,
                &format!("{}   {}", i + 1, group_digits(run.score as usize)),
                pos2(x, y),
                13.0,
                col,
                800,
                Align2::LEFT_TOP,
                None,
            );
            txt(
                &p,
                &fmt_when(run.at, false),
                pos2(x + w, y + px(1.0)),
                12.0,
                th.caption,
                500,
                Align2::RIGHT_TOP,
                None,
            );
            let bar = Rect::from_min_size(pos2(x, y + px(19.0)), vec2(w, px(5.0)));
            paint::inset(&p, bar, None, 2.0, Fill::Base);
            if let Some(acc) = run.acc
                && acc > 0.0
            {
                let m = px(1.0).max(pixels(1.0));
                let fill = Rect::from_min_size(
                    bar.min + vec2(m, m),
                    vec2(
                        trunc_px(ipx(bar.width() - 2.0 * m) * acc).max(bar.height() - 2.0 * m),
                        bar.height() - 2.0 * m,
                    ),
                );
                if i == 0 {
                    paint::glow(&p, fill, th.accent, 4.0, None, None);
                }
                paint::pill(&p, fill, if i == 0 { th.accent } else { th.bar_line });
            }
            y += px(30.0);
        }
        // the last review
        let area = Rect::from_min_max(
            pos2(r.min.x + px(8.0), rev_y - px(8.0)),
            pos2(r.max.x - px(8.0), r.max.y - px(6.0)),
        );
        let resp = last.as_ref().map(|_| {
            interact(ui, area, "home-review", Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand)
        });
        let hovered = resp.as_ref().is_some_and(|r| r.hovered());
        let clicked = resp.as_ref().is_some_and(|r| r.clicked());
        let cr = caps(&p, "LAST REVIEW", pos2(x, rev_y), 11.0, Align2::LEFT_TOP, Some(w));
        if let Some(lr) = &last {
            let when = played_when(lr.at);
            txt(
                &p,
                when.trim_start_matches("played "),
                pos2(x + w, rev_y),
                12.0,
                if hovered { th.accent_ink } else { th.caption },
                500,
                Align2::RIGHT_TOP,
                Some((x + w - cr.max.x - px(12.0)).max(0.0)),
            );
        }
        let mut y = rev_y + head_h;
        for (i, (line, size, weight, col)) in rows.iter().enumerate() {
            if ipx(y + row_h(*size)) > ipx(bottom + px(4.0)) {
                break;
            }
            let col = if i == 0 && hovered { th.accent_ink } else { *col };
            txt(&p, line, pos2(x, y), *size, col, *weight, Align2::LEFT_TOP, None);
            y += row_h(*size);
        }
        clicked
    }

    fn draw_welcome(&mut self, ui: &Ui, lv: &mut LibView, st: &mut AppState, r: Rect) {
        let p = ui.painter().clone();
        let th = t();
        paint::raised(&p, r, Some(30.0), 8.0, Fill::Base, 0.0);
        let (x, y) = (r.min.x + px(40.0), r.min.y + px(30.0));
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
        let line = if st.songs.is_empty() && !st.scanning() {
            songs_note(st)
        } else {
            "Pick a song from your library to start. The last one you played will wait for you here.".into()
        };
        let lines = text::wrap(
            &st.ctx,
            &line,
            Style::new(15.0, 600, th.text_2),
            r.width() - px(80.0),
        );
        let mut ly = y + px(74.0);
        for l in lines.iter().take(3) {
            ly = text::draw(
                &p,
                l,
                pos2(x, ly),
                Align2::LEFT_TOP,
                Style::new(15.0, 600, th.text_2),
                None,
            )
            .max
            .y;
        }
        // the newcomer's way in (P37.6b, D136): the lessons when the build has them, else the easy songs
        let lessons = cfg!(feature = "lessons") && !st.lessons.is_empty();
        // (P37.6a: named for what you play)
        let noun = st.instrument().noun();
        let (new_label, new_view) = if lessons {
            (format!("New to {noun}? Start with Lessons"), View::Lessons)
        } else {
            (format!("New to {noun}? Start with easy songs"), View::Level(0))
        };
        let new_label = new_label.as_str();
        let bh = px(52.0);
        let by = r.max.y - px(30.0) - bh;
        let nw = button_width(&st.ctx, new_label, "", 16.0, false);
        let lw = button_width(&st.ctx, "Open the library", "L", 16.0, false);
        let sw = button_width(&st.ctx, "Set up quickRS", "", 16.0, false);
        let fits = nw + lw + sw + px(28.0) <= r.width() - px(80.0);
        let nb = Rect::from_min_size(pos2(x, by), vec2(nw, bh));
        if button(
            ui,
            nb,
            "home-newcomer",
            new_label,
            "",
            ButtonStyle {
                primary: true,
                size: 16.0,
                r: Some(16.0),
                ..Default::default()
            },
        )
        .clicked()
        {
            lv.go(st, new_view);
        }
        let b = Rect::from_min_size(pos2(nb.max.x + px(14.0), by), vec2(lw, bh));
        if button(
            ui,
            b,
            "home-open-lib",
            "Open the library",
            "L",
            ButtonStyle {
                size: 16.0,
                r: Some(16.0),
                ..Default::default()
            },
        )
        .clicked()
        {
            lv.go(st, View::All);
        }
        if !fits {
            return; // (a narrow window: the setup is in Settings, and on the sidebar's pick)
        }
        // the first-time setup again (P23.14; the getting-started welcome's place, P20.2)
        let g = Rect::from_min_size(pos2(b.max.x + px(14.0), b.min.y), vec2(sw, bh));
        if button(
            ui,
            g,
            "home-welcome",
            "Set up quickRS",
            "",
            ButtonStyle {
                size: 16.0,
                r: Some(16.0),
                // beside the one main action: a text key on the glass look (P34.5)
                plain: true,
                ..Default::default()
            },
        )
        .clicked()
        {
            self.reqs.push(Req::Setup);
        }
    }

    /// One row of covers, each on a raised frame, with title, artist and your best under it; the shelf's tabs
    /// (title, is the starters') along the top. Returns a tab clicked (true: Good to start with).
    #[allow(clippy::too_many_arguments)]
    fn draw_recent(
        &mut self,
        ui: &Ui,
        lv: &mut LibView,
        st: &mut AppState,
        r: Rect,
        songs: &[usize],
        tabs: &[(&str, bool)],
        start: bool,
    ) -> Option<bool> {
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
            return None;
        }
        let size = pixels(size_px as f32);
        let hcy = r.min.y + snap(hdr / 2.0);
        // the tabs: the one showing in the page's ink, the other dimmer, a click switches
        let mut tab_hit: Option<bool> = None;
        let mut tx = r.min.x;
        let mut tr = Rect::from_min_size(pos2(tx, hcy), Vec2::ZERO);
        for (i, (label, is_start)) in tabs.iter().enumerate() {
            let on = *is_start == start;
            let w = r.width() * if tabs.len() > 1 { 0.3 } else { 0.6 };
            let rr = txt(
                &p,
                label,
                pos2(tx, hcy),
                20.0,
                if on { th.text } else { th.caption },
                800,
                Align2::LEFT_CENTER,
                Some(w),
            );
            if tabs.len() > 1 {
                let resp = interact(ui, rr.expand(px(6.0)), ("home-shelf-tab", i), Sense::click());
                if resp.hovered() && !on {
                    st.ctx.set_cursor_icon(egui::CursorIcon::PointingHand);
                }
                if *is_start {
                    resp.clone().on_hover_text(
                        "Easy songs (difficulty 1-3) you haven't played to 80% yet: the library's Difficulty \
                         column says how hard every song is",
                    );
                }
                if resp.clicked() && !on {
                    tab_hit = Some(*is_start);
                }
            }
            tr = rr;
            tx = rr.max.x + px(22.0);
        }
        // Random (P37.10): right after the shelf's title, a quiet key that opens a random song of the whole library
        if random_tool(ui, &st.ctx, pos2(tr.max.x + px(14.0), hcy)).clicked() {
            self.random(lv, st);
        }
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
            let rank = song_rank(&st.userdata, &st.ids(g));
            let tr = txt(
                &p,
                &e.display_title(),
                pos2(x + px(4.0), ty),
                14.0,
                th.text,
                800,
                Align2::LEFT_TOP,
                Some(if rank.is_some() { tw - px(RANK_W) } else { tw }),
            );
            if let Some((tier, acc, part)) = &rank {
                let bc = pos2(tr.max.x + px(11.0), centery(tr));
                draw_rank(&p, bc, *tier);
                if hot && ui.rect_contains_pointer(Rect::from_center_size(bc, vec2(px(20.0), px(20.0)))) {
                    resp.clone().on_hover_text(rank_tip(*acc, part));
                }
            }
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
            let btxt = match levels::song_level(&e, st.instrument().is_bass()) {
                Some(lv_) if start => format!("{} {lv_}/10  ·  {btxt}", qrs_library::PartLevel::word(lv_)),
                _ => btxt,
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
            lv.open_picker(st, g); // (over Home: owner request, 2026-11-02)
        }
        tab_hit
    }

    /// The strip's one-line summary of a Settings page (by its id; the classic look's old ids too).
    fn summary(&self, st: &AppState, page: &str) -> String {
        let cfg = &st.config;
        if page == "timing" {
            // (P35.8, P37.6b: the delay and the low-latency output, the two that change how it feels; P37.6a: named for
            // the instrument, D132)
            let exclusive = st.audio.as_ref().map(|a| a.prefs().exclusive).unwrap_or(false);
            return format!(
                "{} delay {}  ·  low-latency output {}",
                st.instrument().noun_cap(),
                crate::settings::delay_label(cfg.f64("input_offset", 0.0)),
                if exclusive { "on" } else { "off" }
            );
        }
        let all = self.summaries(st);
        let i = match page {
            "sound" | "audio" => 0,
            "tone" => 1,
            "display" => 2,
            "practice" => 3,
            _ => 4,
        };
        all[i].clone()
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
                    .unwrap_or_else(|| "no input".into());
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
                    qrs_audio::tone::ToneSpec::Board { .. } => "Rig".to_string(),
                };
                if a.engine.monitor() {
                    label
                } else {
                    format!("{label}  ·  not hearing your {}", st.instrument().noun())
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
            "Level-up {}  ·  song sync {:+.0} ms",
            if cfg.bool("auto_level", true) { "on" } else { "off" },
            cfg.f64("av_offset", 0.0) * 1000.0
        );
        let n = st.dirs.len();
        let library = format!(
            "{}  ·  {n} folder{}",
            songs(st.groups.len()),
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
        // "Settings F2" opens the settings as F2 does
        let head = Rect::from_min_max(
            pos2(r.min.x + px(10.0), r.min.y + px(10.0)),
            pos2(r.min.x + px(138.0), r.max.y - px(10.0)),
        );
        let hresp = interact(ui, head, "home-set-head", Sense::click());
        if hresp.hovered() {
            paint::raised(&p, head, Some(14.0), 3.0, Fill::Base, 0.0);
            st.ctx.set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        if hresp.clicked() {
            self.reqs.push(Req::Settings("sound"));
        }
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
        // the Settings pages but Tone (the amp room's: its button is at the sidebar's foot), About and Neural Amp
        // Modeler (a special feature, P37.15)
        let cells: Vec<&(&str, &str, &str)> = crate::settings::sections()
            .iter()
            .filter(|(k, ..)| !matches!(*k, "tone" | "about" | "nam"))
            .collect();
        // each cell's width in pixels, not whole: the cells start at int(x0 + i * w) and are int(w) wide
        let w = (ipx(r.max.x - px(10.0)) - ipx(x0)) / cells.len() as f64;
        for (i, (key, label, _)) in cells.into_iter().enumerate() {
            let sum = self.summary(st, key);

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
                &sum,
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
/// "1 song", "1,234 songs".
/// Home's Random key, its left edge at `left` (vertically centred there): the shuffle icon, "Random" and Ctrl R, in
/// secondary ink with a plate only under the pointer (P37.10: quiet, as the library's toolbar tools).
fn random_tool(ui: &Ui, ctx: &egui::Context, left: egui::Pos2) -> egui::Response {
    let th = t();
    let p = ui.painter();
    let lw = text::width(ctx, "Random", Style::new(14.0, 700, th.text_2));
    let kw =
        qrs_ui::keys::keycap_width(ctx, "Ctrl", 11.0) + qrs_ui::keys::keycap_width(ctx, "R", 11.0) + px(3.0);
    let w = px(12.0) + px(18.0) + px(8.0) + lw + px(8.0) + kw + px(12.0);
    let r = Rect::from_min_size(pos2(left.x, left.y - px(17.0)), vec2(w, px(34.0)));
    let resp = interact(ui, r, "home-random", Sense::click());
    let hot = resp.hovered();
    if hot {
        paint::raised(p, r, Some(12.0), 2.0, Fill::Base, 0.0);
        ctx.set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    let col = if hot { th.text } else { th.text_2 };
    let cy = left.y;
    icons::icon(
        p,
        Icon::Shuffle,
        pos2(r.min.x + px(12.0) + px(9.0), cy),
        17.0,
        col,
    );
    let tr = txt(
        p,
        "Random",
        pos2(r.min.x + px(38.0), cy),
        14.0,
        col,
        700,
        Align2::LEFT_CENTER,
        None,
    );
    let kx = keycap(
        p,
        "Ctrl",
        pos2(tr.max.x + px(8.0), cy),
        Align2::LEFT_CENTER,
        11.0,
        CapOn::Material,
    )
    .max
    .x;
    keycap(
        p,
        "R",
        pos2(kx + px(3.0), cy),
        Align2::LEFT_CENTER,
        11.0,
        CapOn::Material,
    );
    resp.on_hover_text("Open a random song from your whole library (Ctrl+R)")
}

pub fn songs(n: usize) -> String {
    format!("{} song{}", group_digits(n), if n == 1 { "" } else { "s" })
}

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
