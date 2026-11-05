//! A song's difficulty and parts in the library (P37.6b, D137): the level the index keeps for each part
//! (`qrs_library::difficulty`), the song's level (its easiest main part, or its bass part on a bass), the parts it
//! has as letters (L R C B), the difficulty bands the sidebar filters by and the pips the rows draw.

use eframe::egui;
use egui::{Color32, Painter, Pos2, Rect, Vec2, pos2, vec2};
use qrs_library::{PartEntry, PartLevel, SongEntry};
use qrs_ui::theme::{self, t};
use qrs_ui::units::px;

/// The sidebar's difficulty bands: (name, lowest level, highest level).
pub const BANDS: [(&str, u8, u8); 3] = [("Easy", 1, 3), ("Medium", 4, 6), ("Hard", 7, 10)];

/// The level a band counts as "good to start with" up to (Home's shelf).
pub const STARTER_MAX: u8 = 3;

fn is_bass(a: &PartEntry) -> bool {
    a.name.to_lowercase().starts_with("bass")
}

/// The song's level: on a bass its bass part's, otherwise its easiest main guitar part's (bonus parts only when
/// there's nothing else). None while the library hasn't worked it out (or the charts couldn't be read).
pub fn song_level(e: &SongEntry, bass: bool) -> Option<u8> {
    let lv = |a: &PartEntry| a.level.as_ref().map(|l| l.level);
    if bass {
        let b = super::bass_part(e).and_then(lv);
        if b.is_some() {
            return b;
        }
    }
    let main = e
        .arrangements
        .iter()
        .filter(|a| !is_bass(a) && !a.bonus)
        .filter_map(lv)
        .min();
    main.or_else(|| e.arrangements.iter().filter_map(lv).min())
}

/// The part whose level is the song's (for its tooltip's details).
pub fn level_part(e: &SongEntry, bass: bool) -> Option<&PartEntry> {
    let want = song_level(e, bass)?;
    let mut cands: Vec<&PartEntry> = e.arrangements.iter().collect();
    // the same order song_level looks in: bass first on a bass, main parts before bonus ones
    cands.sort_by_key(|a| (!(bass && is_bass(a)), is_bass(a) && !bass, a.bonus));
    cands
        .into_iter()
        .find(|a| a.level.as_ref().is_some_and(|l| l.level == want))
}

/// The kinds of part a song has, as letters in a fixed order: L(ead), R(hythm), C(ombo), B(ass).
pub fn part_letters(e: &SongEntry) -> Vec<char> {
    let mut out = Vec::new();
    for (c, word) in [('L', "lead"), ('R', "rhythm"), ('C', "combo"), ('B', "bass")] {
        if e.arrangements
            .iter()
            .any(|a| a.name.to_lowercase().starts_with(word))
        {
            out.push(c);
        }
    }
    out
}

/// The parts' tooltip: "Lead: 6/10 · Rhythm: 3/10 · Bass: 2/10" (bonus parts marked).
pub fn parts_tip(e: &SongEntry) -> String {
    let lines: Vec<String> = e
        .arrangements
        .iter()
        .map(|a| {
            let bonus = if a.bonus { " (bonus)" } else { "" };
            match &a.level {
                Some(l) => format!("{}{bonus}: {}/10", a.name, l.level),
                None => format!("{}{bonus}", a.name),
            }
        })
        .collect();
    lines.join("  ·  ")
}

/// The difficulty's tooltip: "Easy (2/10), Rhythm: 1.4 notes a second, up to fret 5, ..." and whether the chart
/// can be played easier.
pub fn level_tip(e: &SongEntry, bass: bool) -> String {
    let Some(a) = level_part(e, bass) else {
        return "Difficulty: worked out when the library reads the song".into();
    };
    let Some(l) = a.level.as_ref() else {
        return String::new();
    };
    let mut s = format!(
        "{} ({}/10), {}: {}",
        PartLevel::word(l.level),
        l.level,
        a.name,
        l.summary()
    );
    if l.dd {
        s += ". Has difficulty levels: Easier in Controls thins it out";
    }
    s
}

/// The level's colour: green (easy) through amber to red (expert).
pub fn level_color(level: u8) -> Color32 {
    let th = t();
    match level {
        0..=3 => th.hit,
        4..=6 => th.accent_ink,
        7..=8 => th.late,
        _ => th.miss,
    }
}

/// Five pips for a level (two levels each), the lit ones in the level's colour; returns where they went.
pub fn draw_pips(p: &Painter, at: Pos2, level: u8) -> Rect {
    let th = t();
    let (r, gap) = (px(3.5), px(4.0));
    let lit = level.div_ceil(2) as usize;
    let col = level_color(level);
    let dim = if theme::is_dark() { th.line } else { th.tick };
    for i in 0..5 {
        let c = pos2(at.x + r + i as f32 * (2.0 * r + gap), at.y);
        p.circle_filled(c, r, if i < lit { col } else { dim });
    }
    Rect::from_min_size(pos2(at.x, at.y - r), vec2(10.0 * r + 4.0 * gap, 2.0 * r))
}

/// The size of one part letter's chip.
pub fn letter_size() -> Vec2 {
    vec2(px(16.0), px(18.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn part(name: &str, bonus: bool, level: Option<u8>) -> PartEntry {
        PartEntry {
            key: name.to_lowercase(),
            name: name.into(),
            bonus,
            level: level.map(|level| PartLevel {
                level,
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    #[test]
    fn a_songs_level_is_its_easiest_main_part_or_its_bass_part_on_a_bass() {
        let e = SongEntry {
            arrangements: vec![
                part("Lead", false, Some(7)),
                part("Rhythm", false, Some(4)),
                part("Rhythm", true, Some(2)),
                part("Bass", false, Some(3)),
            ],
            ..Default::default()
        };
        assert_eq!(song_level(&e, false), Some(4)); // (not the bonus part, not the bass)
        assert_eq!(song_level(&e, true), Some(3));
        assert_eq!(level_part(&e, false).unwrap().name, "Rhythm");
        assert!(!level_part(&e, false).unwrap().bonus);
        assert_eq!(part_letters(&e), vec!['L', 'R', 'B']);
        assert_eq!(
            parts_tip(&e),
            "Lead: 7/10  ·  Rhythm: 4/10  ·  Rhythm (bonus): 2/10  ·  Bass: 3/10"
        );
        assert!(level_tip(&e, false).starts_with("Medium (4/10), Rhythm: "));
        // only a bonus part: it counts
        let b = SongEntry {
            arrangements: vec![part("Lead", true, Some(5))],
            ..Default::default()
        };
        assert_eq!(song_level(&b, false), Some(5));
        // not worked out yet
        let n = SongEntry {
            arrangements: vec![part("Lead", false, None)],
            ..Default::default()
        };
        assert_eq!(song_level(&n, false), None);
        assert!(level_tip(&n, false).starts_with("Difficulty: worked out"));
    }

    #[test]
    fn the_bands_cover_every_level_once() {
        for lv in 1..=10u8 {
            assert_eq!(BANDS.iter().filter(|b| (b.1..=b.2).contains(&lv)).count(), 1);
        }
        assert_eq!(BANDS[0].2, STARTER_MAX);
    }
}
