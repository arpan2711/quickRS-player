//! Lyrics grouped into display lines. Port of rsplay/lyrics.py.
//!
//! Rocksmith (and feedpak) store lyrics as timed syllables: a trailing "-" joins the syllable to the next one
//! without a space, and a trailing "+" ends the line.

#[derive(Clone, Debug, PartialEq)]
pub struct Syllable {
    pub time: f64,
    pub end: f64,
    /// As displayed, including the following space when there is one.
    pub text: String,
    /// MIDI pitch when the source has one (Rocksmith vocals); None otherwise.
    pub pitch: Option<i32>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Line {
    pub syllables: Vec<Syllable>,
}

impl Line {
    pub fn start(&self) -> f64 {
        self.syllables.first().map(|s| s.time).unwrap_or(0.0)
    }
    pub fn end(&self) -> f64 {
        self.syllables.last().map(|s| s.end).unwrap_or(0.0)
    }
    pub fn text(&self) -> String {
        self.syllables
            .iter()
            .map(|s| s.text.as_str())
            .collect::<String>()
            .trim_end()
            .to_string()
    }
}

/// Seconds of silence that also end a line.
pub const MAX_GAP: f64 = 4.0;

/// A raw timed syllable from a chart.
#[derive(Clone, Debug)]
pub struct RawSyllable {
    pub time: f64,
    pub length: f64,
    pub text: String,
    pub pitch: Option<i32>,
}

pub fn build_lines(vocals: &[RawSyllable]) -> Vec<Line> {
    let mut sorted: Vec<&RawSyllable> = vocals.iter().collect();
    sorted.sort_by(|a, b| a.time.total_cmp(&b.time));
    let mut lines = Vec::new();
    let mut cur = Line::default();
    for v in sorted {
        let raw = v.text.trim();
        if raw.is_empty() {
            continue;
        }
        if !cur.syllables.is_empty() && v.time - cur.end() > MAX_GAP {
            lines.push(std::mem::take(&mut cur));
        }
        let ends_line = raw.ends_with('+');
        let raw = raw.trim_end_matches('+');
        let joins = raw.ends_with('-');
        let text = if joins {
            raw[..raw.len() - 1].to_string()
        } else {
            format!("{raw} ")
        };
        cur.syllables.push(Syllable {
            time: v.time,
            end: v.time + v.length.max(0.05),
            text,
            pitch: v.pitch,
        });
        if ends_line {
            lines.push(std::mem::take(&mut cur));
        }
    }
    if !cur.syllables.is_empty() {
        lines.push(cur);
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(time: f64, text: &str) -> RawSyllable {
        RawSyllable {
            time,
            length: 0.3,
            text: text.into(),
            pitch: None,
        }
    }

    #[test]
    fn joins_and_line_ends() {
        let lines = build_lines(&[s(0.0, "Hel-"), s(0.3, "lo"), s(0.6, "world+"), s(1.0, "next")]);
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].text(), "Hello world");
        assert_eq!(lines[1].text(), "next");
    }

    #[test]
    fn long_gap_ends_a_line() {
        let lines = build_lines(&[s(0.0, "a"), s(10.0, "b"), s(5.0, " ")]);
        assert_eq!(lines.len(), 2);
    }
}
