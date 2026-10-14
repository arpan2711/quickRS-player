//! Case-insensitive glob matching like Python's `fnmatch.fnmatchcase` on lowercased names: `*` matches any run
//! of characters including `/` (so `songs/bin/*.sng` also finds `songs/bin/generic/x.sng`), `?` one character,
//! `[...]` a set (`[!...]` negated).

pub fn matches(pattern: &str, name: &str) -> bool {
    let p: Vec<char> = pattern.to_lowercase().chars().collect();
    let n: Vec<char> = name.to_lowercase().chars().collect();
    match_from(&p, &n)
}

fn match_from(p: &[char], n: &[char]) -> bool {
    // Iterative wildcard matching with backtracking to the last '*'.
    let (mut pi, mut ni) = (0usize, 0usize);
    let mut star: Option<(usize, usize)> = None;
    while ni < n.len() {
        if pi < p.len() {
            match p[pi] {
                '*' => {
                    star = Some((pi, ni));
                    pi += 1;
                    continue;
                }
                '?' => {
                    pi += 1;
                    ni += 1;
                    continue;
                }
                '[' => {
                    if let Some((ok, len)) = class(&p[pi..], n[ni])
                        && ok
                    {
                        pi += len;
                        ni += 1;
                        continue;
                    } else if class(&p[pi..], n[ni]).is_none() && n[ni] == '[' {
                        pi += 1;
                        ni += 1;
                        continue;
                    }
                }
                c if c == n[ni] => {
                    pi += 1;
                    ni += 1;
                    continue;
                }
                _ => {}
            }
        }
        match star {
            Some((sp, sn)) => {
                pi = sp + 1;
                ni = sn + 1;
                star = Some((sp, sn + 1));
            }
            None => return false,
        }
    }
    p[pi..].iter().all(|c| *c == '*')
}

/// `[...]` at the start of `p`: Some((matches, length of the class)) or None when it isn't a closed class.
fn class(p: &[char], c: char) -> Option<(bool, usize)> {
    let mut i = 1;
    let negate = i < p.len() && p[i] == '!';
    if negate {
        i += 1;
    }
    let start = i;
    let mut hit = false;
    while i < p.len() {
        if p[i] == ']' && i > start {
            return Some((hit != negate, i + 1));
        }
        if i + 2 < p.len() && p[i + 1] == '-' && p[i + 2] != ']' {
            if p[i] <= c && c <= p[i + 2] {
                hit = true;
            }
            i += 3;
        } else {
            if p[i] == c {
                hit = true;
            }
            i += 1;
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::matches;

    #[test]
    fn star_crosses_slashes_and_ignores_case() {
        assert!(matches("songs/bin/*.sng", "songs/bin/generic/Demo_Lead.SNG"));
        assert!(matches("manifests/*.json", "manifests/songs_dlc_x/x_lead.json"));
        assert!(matches("*.wem", "audio/windows/123.wem"));
        assert!(!matches("songs/arr/*.xml", "songs/bin/generic/a.sng"));
        assert!(matches("songs/bin/*vocals.sng", "songs/bin/generic/x_vocals.sng"));
    }

    #[test]
    fn question_mark_and_classes() {
        assert!(matches("a?c", "abc"));
        assert!(matches("[a-c]x", "bx"));
        assert!(!matches("[!a-c]x", "bx"));
        assert!(matches("*", ""));
        assert!(!matches("a", ""));
    }
}
