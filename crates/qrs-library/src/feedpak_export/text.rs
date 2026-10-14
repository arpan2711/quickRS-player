//! The exporter's text, byte for byte as the Python exporter writes it: readable JSON (`to_json`: anything that
//! fits on a line stays on one, so one note per line; Python's ", " and ": " separators and float repr) and
//! manifest.yaml (port of rsfeedpak/manifest_yaml.py: block style, every string double-quoted with JSON escapes,
//! floats always with a decimal point, short lists of scalars in flow style).

use serde_json::{Map, Value};

/// A float as Python's `repr` writes it (the shortest digits that read back; "12.0", "0.001", "1e-05").
pub fn py_float(v: f64) -> String {
    if v.is_nan() {
        return "NaN".into();
    }
    if v.is_infinite() {
        return if v > 0.0 {
            "Infinity".into()
        } else {
            "-Infinity".into()
        };
    }
    let a = v.abs();
    if a != 0.0 && !(1e-4..1e16).contains(&a) {
        // Python switches to exponent notation here: 1e-05, 1.5e+16
        let s = format!("{v:e}"); // e.g. "1e-5", "1.5e16"
        let (mant, exp) = s.split_once('e').unwrap_or((&s, "0"));
        let e: i32 = exp.parse().unwrap_or(0);
        return format!("{mant}e{}{:02}", if e < 0 { '-' } else { '+' }, e.abs());
    }
    let s = format!("{v}");
    if s.contains('.') { s } else { format!("{s}.0") }
}

fn number(n: &serde_json::Number) -> String {
    if let Some(i) = n.as_i64() {
        return i.to_string();
    }
    if let Some(u) = n.as_u64() {
        return u.to_string();
    }
    py_float(n.as_f64().unwrap_or(0.0))
}

fn string(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_else(|_| "\"\"".into())
}

/// `json.dumps(obj, ensure_ascii=False, separators=(", ", ": "))`
pub fn compact(v: &Value) -> String {
    let mut out = String::new();
    write_compact(v, &mut out);
    out
}

fn write_compact(v: &Value, out: &mut String) {
    match v {
        Value::Null => out.push_str("null"),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Number(n) => out.push_str(&number(n)),
        Value::String(s) => out.push_str(&string(s)),
        Value::Array(a) => {
            out.push('[');
            for (i, x) in a.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                write_compact(x, out);
            }
            out.push(']');
        }
        Value::Object(m) => {
            out.push('{');
            for (i, (k, x)) in m.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                out.push_str(&string(k));
                out.push_str(": ");
                write_compact(x, out);
            }
            out.push('}');
        }
    }
}

/// Readable JSON for hand editing: anything that fits in `width` stays on one line (export.py `to_json`).
pub fn to_json(v: &Value, indent: usize, width: usize) -> String {
    let flat = compact(v);
    let container =
        matches!(v, Value::Array(a) if !a.is_empty()) || matches!(v, Value::Object(m) if !m.is_empty());
    if flat.chars().count() + indent <= width || !container {
        return flat;
    }
    let (pad, inner) = (" ".repeat(indent), " ".repeat(indent + 2));
    match v {
        Value::Object(m) => {
            let items: Vec<String> = m
                .iter()
                .map(|(k, x)| {
                    format!(
                        "{inner}{}: {}",
                        string(k),
                        to_json(x, indent + 2, width).trim_start()
                    )
                })
                .collect();
            format!("{{\n{}\n{pad}}}", items.join(",\n"))
        }
        Value::Array(a) => {
            let items: Vec<String> = a
                .iter()
                .map(|x| format!("{inner}{}", to_json(x, indent + 2, width).trim_start()))
                .collect();
            format!("[\n{}\n{pad}]", items.join(",\n"))
        }
        _ => flat,
    }
}

// -- manifest.yaml ------------------------------------------------------------------------------------------
/// Characters YAML won't take literally even inside quotes (and the ones it reads as line breaks).
fn unsafe_char(c: char) -> bool {
    let u = c as u32;
    !(u == 0x09
        || u == 0x0a
        || u == 0x0d
        || (0x20..=0x7e).contains(&u)
        || (0xa0..=0x2027).contains(&u)
        || (0x202a..=0xd7ff).contains(&u)
        || (0xe000..=0xfffd).contains(&u)
        || (0x10000..=0x10ffff).contains(&u))
}

fn yaml_scalar(v: &Value) -> Result<String, String> {
    Ok(match v {
        Value::Null => "null".into(),
        Value::Bool(b) => if *b { "true" } else { "false" }.into(),
        Value::Number(n) => {
            if n.as_i64().is_some() || n.as_u64().is_some() {
                number(n)
            } else {
                let f = n.as_f64().unwrap_or(0.0);
                if !f.is_finite() {
                    return Err(format!("can't write {f} to a manifest"));
                }
                let mut s = py_float(f);
                if s.contains('e') || s.contains('E') {
                    s = format!("{f:.9}").trim_end_matches('0').to_string();
                }
                if s.ends_with('.') {
                    s + "0"
                } else if s.contains('.') {
                    s
                } else {
                    s + ".0"
                }
            }
        }
        Value::String(s) => {
            let j = string(s);
            let mut out = String::with_capacity(j.len());
            for c in j.chars() {
                if unsafe_char(c) {
                    out.push_str(&format!("\\u{:04x}", c as u32));
                } else {
                    out.push(c);
                }
            }
            out
        }
        _ => return Err("not a scalar".into()),
    })
}

fn is_scalar(v: &Value) -> bool {
    !matches!(v, Value::Array(_) | Value::Object(_))
}

fn key_ok(k: &str) -> bool {
    let mut cs = k.chars();
    matches!(cs.next(), Some(c) if c.is_ascii_alphabetic() || c == '_')
        && cs.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn yaml_lines(obj: &Map<String, Value>, indent: usize) -> Result<Vec<String>, String> {
    let pad = " ".repeat(indent);
    let mut out = Vec::new();
    for (k, v) in obj {
        if !key_ok(k) {
            return Err(format!("unsupported manifest key {k:?}"));
        }
        match v {
            _ if is_scalar(v) => out.push(format!("{pad}{k}: {}", yaml_scalar(v)?)),
            Value::Object(m) if m.is_empty() => out.push(format!("{pad}{k}: {{}}")),
            Value::Object(m) => {
                out.push(format!("{pad}{k}:"));
                out.extend(yaml_lines(m, indent + 2)?);
            }
            Value::Array(a) if a.is_empty() => out.push(format!("{pad}{k}: []")),
            Value::Array(a) if a.iter().all(is_scalar) => {
                let items: Result<Vec<String>, String> = a.iter().map(yaml_scalar).collect();
                out.push(format!("{pad}{k}: [{}]", items?.join(", ")));
            }
            Value::Array(a) => {
                out.push(format!("{pad}{k}:"));
                for item in a {
                    let Value::Object(m) = item else {
                        return Err("lists in a manifest hold scalars or non-empty mappings".into());
                    };
                    if m.is_empty() {
                        return Err("lists in a manifest hold scalars or non-empty mappings".into());
                    }
                    let mut lines = yaml_lines(m, indent + 4)?;
                    lines[0] = format!("{pad}  - {}", &lines[0][indent + 4..]);
                    out.extend(lines);
                }
            }
            _ => return Err("can't write that to a manifest".into()),
        }
    }
    Ok(out)
}

/// The manifest as YAML text (no BOM, a trailing newline), with `header` as comment lines.
pub fn manifest_yaml(manifest: &Map<String, Value>, header: &str) -> Result<String, String> {
    let mut head = String::new();
    for line in header.lines() {
        if line.is_empty() {
            head.push_str("#\n");
        } else {
            head.push_str(&format!("# {line}\n"));
        }
    }
    Ok(head + &yaml_lines(manifest, 0)?.join("\n") + "\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn floats_read_like_python() {
        assert_eq!(py_float(12.0), "12.0");
        assert_eq!(py_float(0.001), "0.001");
        assert_eq!(py_float(12.345), "12.345");
        assert_eq!(py_float(-15.0), "-15.0");
        assert_eq!(py_float(0.0), "0.0");
        assert_eq!(py_float(1e-5), "1e-05");
        assert_eq!(py_float(1.5e16), "1.5e+16");
        assert_eq!(py_float(0.1 + 0.2), "0.30000000000000004");
    }

    #[test]
    fn json_layout_matches_export_py() {
        let v = json!({"notes": [{"t": 1.5, "s": 0, "f": 3}, {"t": 2.0, "s": 1, "f": 5, "ho": true}], "name": "Lead"});
        assert_eq!(
            compact(&v),
            r#"{"notes": [{"t": 1.5, "s": 0, "f": 3}, {"t": 2.0, "s": 1, "f": 5, "ho": true}], "name": "Lead"}"#
        );
        // narrow: one item per line, nested indents of two (a 38-char note at indent 4 fits 42, not 41)
        let narrow = to_json(&v, 0, 42);
        assert_eq!(
            narrow,
            "{\n  \"notes\": [\n    {\"t\": 1.5, \"s\": 0, \"f\": 3},\n    {\"t\": 2.0, \"s\": 1, \"f\": 5, \"ho\": true}\n  ],\n  \"name\": \"Lead\"\n}"
        );
        assert_eq!(to_json(&json!([]), 0, 1), "[]");
        assert_eq!(
            compact(&json!("Beyonc\u{e9} \"x\"")),
            "\"Beyonc\u{e9} \\\"x\\\"\""
        );
    }

    #[test]
    fn yaml_like_manifest_yaml_py() {
        let m = json!({
            "feedpak_version": "1.19.0",
            "title": "Yes",
            "year": 1979,
            "duration": 245.5,
            "tempo": 1e-5,
            "arrangements": [{"id": "lead", "tuning": [0, 0, 0, 0, 0, 0], "rocksmith": {"bonus": false}}],
            "stems": [],
            "odd": "a\u{2028}b"
        });
        let y = manifest_yaml(m.as_object().unwrap(), "A - B\n\nline").unwrap();
        assert_eq!(
            y,
            "# A - B\n#\n# line\nfeedpak_version: \"1.19.0\"\ntitle: \"Yes\"\nyear: 1979\nduration: 245.5\n\
             tempo: 0.00001\narrangements:\n  - id: \"lead\"\n    tuning: [0, 0, 0, 0, 0, 0]\n    rocksmith:\n      \
             bonus: false\nstems: []\nodd: \"a\\u2028b\"\n"
        );
        assert!(manifest_yaml(json!({"bad key": 1}).as_object().unwrap(), "").is_err());
    }
}
