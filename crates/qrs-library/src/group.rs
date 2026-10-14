//! One row per song when the same song is in the library twice (a psarc and its converted feedpak, PLAN.md
//! section 5): a feedpak whose provenance names a psarc joins that psarc; others join a song with the same
//! normalized artist and title whose length is within 2 seconds. The preferred source plays (psarc when there are
//! keys, else the feedpak, or what Settings > Library says).

use std::collections::HashMap;

use crate::scan::SongEntry;

/// Lengths closer than this are the same recording.
pub const SAME_LENGTH: f64 = 2.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Prefer {
    Psarc,
    Feedpak,
}

impl Prefer {
    pub fn key(self) -> &'static str {
        match self {
            Prefer::Psarc => "psarc",
            Prefer::Feedpak => "feedpak",
        }
    }

    pub fn parse(s: &str) -> Option<Prefer> {
        match s {
            "psarc" => Some(Prefer::Psarc),
            "feedpak" => Some(Prefer::Feedpak),
            _ => None,
        }
    }
}

/// A song as the library lists it: its sources (indexes into the song list), the one that plays, and the id its
/// history is shared under.
#[derive(Clone, Debug, PartialEq)]
pub struct Group {
    pub members: Vec<usize>,
    pub preferred: usize,
    /// "artist|title", normalized.
    pub canonical: String,
}

impl Group {
    /// Every source's Python id.
    pub fn ids(&self, songs: &[SongEntry]) -> Vec<String> {
        self.members.iter().map(|&i| songs[i].id()).collect()
    }

    pub fn has_psarc(&self, songs: &[SongEntry]) -> bool {
        self.members.iter().any(|&i| !songs[i].is_feedpak())
    }

    pub fn has_feedpak(&self, songs: &[SongEntry]) -> bool {
        self.members.iter().any(|&i| songs[i].is_feedpak())
    }

    /// "PSARC · FEEDPAK"
    pub fn sources_label(&self, songs: &[SongEntry]) -> String {
        let mut parts = Vec::new();
        if self.has_psarc(songs) {
            parts.push("PSARC");
        }
        if self.has_feedpak(songs) {
            parts.push("FEEDPAK");
        }
        parts.join(" · ")
    }
}

/// Lower case, letters and digits only, single spaces ("The Beatles " and "the beatles" match; "AC/DC" is "ac dc").
pub fn normalize(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut space = false;
    for c in s.chars().flat_map(char::to_lowercase) {
        if c.is_alphanumeric() {
            if space && !out.is_empty() {
                out.push(' ');
            }
            out.push(c);
            space = false;
        } else {
            space = true;
        }
    }
    out
}

pub fn canonical_id(e: &SongEntry) -> String {
    let (a, t) = (normalize(&e.artist), normalize(&e.display_title()));
    if a.is_empty() && t.is_empty() {
        String::new()
    } else {
        format!("{a}|{t}")
    }
}

/// Group the songs. Entries that failed to read stand alone. Groups come out in the order of their first member.
pub fn group(songs: &[SongEntry], prefer: Prefer) -> Vec<Group> {
    let mut groups: Vec<Group> = Vec::new();
    let mut by_name: HashMap<String, Vec<usize>> = HashMap::new(); // canonical -> group indexes
    let mut by_id: HashMap<String, usize> = HashMap::new(); // psarc id -> group index
    // psarcs first, so feedpaks can find their source by id
    let mut order: Vec<usize> = (0..songs.len()).collect();
    order.sort_by_key(|&i| (songs[i].is_feedpak(), i));
    for i in order {
        let e = &songs[i];
        let canon = canonical_id(e);
        let mut target: Option<usize> = None;
        if e.is_feedpak() && !e.source_id.is_empty() {
            target = by_id.get(&e.source_id).copied();
        }
        if target.is_none() && e.error.is_empty() && !canon.is_empty() {
            target = by_name.get(&canon).and_then(|gs| {
                gs.iter().copied().find(|&g| {
                    let m = &songs[groups[g].members[0]];
                    (m.length - e.length).abs() <= SAME_LENGTH && m.is_feedpak() != e.is_feedpak()
                })
            });
        }
        match target {
            Some(g) => groups[g].members.push(i),
            None => {
                groups.push(Group {
                    members: vec![i],
                    preferred: i,
                    canonical: canon.clone(),
                });
                let g = groups.len() - 1;
                if e.error.is_empty() && !canon.is_empty() {
                    by_name.entry(canon).or_default().push(g);
                }
            }
        }
        if !e.is_feedpak() {
            let g = target.unwrap_or(groups.len() - 1);
            by_id.insert(e.id(), g);
        }
    }
    for g in &mut groups {
        g.members.sort_unstable();
        g.preferred = pick(songs, &g.members, prefer);
    }
    groups.sort_by_key(|g| g.members[0]);
    groups
}

/// The member that plays: a playable one of the preferred format, else any playable one, else the first.
fn pick(songs: &[SongEntry], members: &[usize], prefer: Prefer) -> usize {
    let want_feedpak = prefer == Prefer::Feedpak;
    members
        .iter()
        .copied()
        .find(|&i| songs[i].playable() && songs[i].is_feedpak() == want_feedpak)
        .or_else(|| members.iter().copied().find(|&i| songs[i].playable()))
        .unwrap_or(members[0])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn song(path: &str, fmt: &str, artist: &str, title: &str, len: f64) -> SongEntry {
        SongEntry {
            path: path.into(),
            artist: artist.into(),
            title: title.into(),
            length: len,
            format: fmt.into(),
            arrangements: vec![crate::scan::PartEntry {
                key: "lead".into(),
                name: "Lead".into(),
                tuning: "E Standard".into(),
                bonus: false,
            }],
            ..Default::default()
        }
    }

    #[test]
    fn twins_group_by_provenance_or_name() {
        let mut songs = vec![
            song(r"d\abc_p.psarc", "psarc", "AC/DC", "Back In Black", 255.0),
            song(
                r"f\AC-DC - Back in Black.feedpak",
                "feedpak",
                "AC/DC",
                "Back in Black",
                255.4,
            ),
            song(r"d\other_p.psarc", "psarc", "Band", "Song", 200.0),
            song(r"f\renamed.feedpak", "feedpak", "Band X", "Song X", 10.0),
            song(r"f\Band - Song (live).feedpak", "feedpak", "Band", "Song", 260.0),
        ];
        songs[3].source_id = "other_p.psarc|".into();
        let g = group(&songs, Prefer::Psarc);
        assert_eq!(g.len(), 3, "{g:?}");
        assert_eq!(g[0].members, vec![0, 1]);
        assert_eq!(g[0].preferred, 0);
        assert_eq!(g[0].canonical, "ac dc|back in black");
        assert_eq!(g[1].members, vec![2, 3]); // by provenance, whatever its name
        assert_eq!(g[2].members, vec![4]); // a different recording
        assert_eq!(g[0].sources_label(&songs), "PSARC · FEEDPAK");
        let g = group(&songs, Prefer::Feedpak);
        assert_eq!(g[0].preferred, 1);
        // a psarc that needs keys never plays when its feedpak can
        songs[0].needs_keys = true;
        let g = group(&songs, Prefer::Psarc);
        assert_eq!(g[0].preferred, 1);
    }

    #[test]
    fn normalizing() {
        assert_eq!(normalize("  The Beatles "), "the beatles");
        assert_eq!(normalize("Guns N' Roses"), "guns n roses");
        assert_eq!(normalize("Mötley Crüe"), "mötley crüe");
    }
}
