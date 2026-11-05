//! An RS tone's rig: the amp, cab, pedals and racks (with their knob values) that a song's manifest lists
//! for each tone of a part (`Attributes.Tones` in a .psarc manifest; feedpak keeps the same objects under the
//! arrangement's `tones.definitions`). The pedalboard turns them into boards (qrs-audio `board::rs_gear`).

use serde_json::Value;

/// One piece of gear in a tone.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Gear {
    /// "PrePedal1".."PrePedal3", "Amp", "PostPedal1".."PostPedal3", "Cabinet", "Rack1".."Rack3".
    pub slot: String,
    /// e.g. "Amp_MarshallJCM800", "Pedal_SpringReverb", "Cab_Marshall1960a_57_Edge".
    pub key: String,
    /// Knob values by name, the gear key prefix taken off ("Gain", "Treble", "2200").
    pub knobs: Vec<(String, f32)>,
}

impl Gear {
    pub fn knob(&self, name: &str) -> Option<f32> {
        self.knobs
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| *v)
    }
}

/// One tone of a part.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RigTone {
    /// The tone's key, as the chart's tone changes name it (e.g. "AudBe_cleanlead").
    pub key: String,
    pub name: String,
    /// "CLEAN", "LEAD", "DISTORTION"... (the localisation id taken off).
    pub descriptors: Vec<String>,
    /// RS's loudness trim for the tone, dB (0 when missing).
    pub volume_db: f32,
    /// In signal order: pre pedals, amp, post (loop) pedals, cab, racks.
    pub gear: Vec<Gear>,
}

/// RS's slots in signal order.
pub const SLOTS: [&str; 11] = [
    "PrePedal1",
    "PrePedal2",
    "PrePedal3",
    "Amp",
    "PostPedal1",
    "PostPedal2",
    "PostPedal3",
    "Cabinet",
    "Rack1",
    "Rack2",
    "Rack3",
];

fn num(v: Option<&Value>) -> Option<f32> {
    v.and_then(|v| {
        v.as_f64()
            .or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()))
    })
    .map(|f| f as f32)
}

impl RigTone {
    /// A tone object of a manifest (None when it has no key).
    pub fn from_json(v: &Value) -> Option<RigTone> {
        let key = v.get("Key").and_then(Value::as_str).unwrap_or("").to_string();
        let name = v.get("Name").and_then(Value::as_str).unwrap_or("").to_string();
        if key.is_empty() && name.is_empty() {
            return None;
        }
        let descriptors = v
            .get("ToneDescriptors")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(Value::as_str)
                    .map(|d| d.rsplit(']').next().unwrap_or(d).trim().to_uppercase())
                    .filter(|d| !d.is_empty())
                    .collect()
            })
            .unwrap_or_default();
        let mut gear = Vec::new();
        if let Some(list) = v.get("GearList").and_then(Value::as_object) {
            for slot in SLOTS {
                let Some(g) = list.get(slot) else { continue };
                let Some(gkey) = g.get("Key").and_then(Value::as_str).filter(|k| !k.is_empty()) else {
                    continue;
                };
                let prefix = format!("{gkey}_");
                let mut knobs: Vec<(String, f32)> = g
                    .get("KnobValues")
                    .and_then(Value::as_object)
                    .map(|m| {
                        m.iter()
                            .filter_map(|(k, x)| {
                                let short = k.strip_prefix(&prefix).unwrap_or(k);
                                num(Some(x)).map(|f| (short.to_string(), f))
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                knobs.sort_by(|a, b| a.0.cmp(&b.0));
                gear.push(Gear {
                    slot: slot.to_string(),
                    key: gkey.to_string(),
                    knobs,
                });
            }
        }
        Some(RigTone {
            key: if key.is_empty() { name.clone() } else { key },
            name,
            descriptors,
            volume_db: num(v.get("Volume")).unwrap_or(0.0),
            gear,
        })
    }

    pub fn has(&self, descriptor: &str) -> bool {
        self.descriptors.iter().any(|d| d == descriptor)
    }

    /// A short label for the tone from its key: what follows the song's prefix ("AudBe_wahsolo" → "wah solo").
    pub fn label(&self) -> String {
        let raw = self.key.rsplit('_').next().unwrap_or(&self.key).to_lowercase();
        const WORDS: [&str; 30] = [
            "breakdown",
            "acoustic",
            "chorus",
            "phaser",
            "flanger",
            "octave",
            "filter",
            "clean",
            "drive",
            "intro",
            "outro",
            "verse",
            "bridge",
            "solo",
            "lead",
            "dist",
            "fuzz",
            "trem",
            "verb",
            "echo",
            "slide",
            "keys",
            "low",
            "high",
            "wah",
            "mod",
            "ac",
            "rhythm",
            "crunch",
            "delay",
        ];
        // split the run of words greedily, longest first
        let mut out: Vec<String> = Vec::new();
        let mut rest = raw.as_str();
        'outer: while !rest.is_empty() {
            for w in WORDS {
                if let Some(r) = rest.strip_prefix(w) {
                    out.push(if w == "ac" {
                        "acoustic".into()
                    } else {
                        w.to_string()
                    });
                    rest = r;
                    continue 'outer;
                }
            }
            // an unknown run: keep it as one word
            out.push(rest.to_string());
            break;
        }
        let s = out.join(" ");
        if s.is_empty() { self.name.clone() } else { s }
    }
}

/// Every distinct tone in a list of tone objects (a part's manifest lists its base tone twice).
pub fn parse_tones(v: Option<&Value>) -> Vec<RigTone> {
    let mut out: Vec<RigTone> = Vec::new();
    if let Some(a) = v.and_then(Value::as_array) {
        for t in a.iter().filter_map(RigTone::from_json) {
            if !out.iter().any(|o| o.key == t.key) {
                out.push(t);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_manifest_tone_reads() {
        let v: Value = serde_json::from_str(
            r#"{"GearList":{"Rack1":{"Type":"Racks","KnobValues":{"Rack_StudioEQ_Bass":-1.0,"Rack_StudioEQ_BassFreq":314.0},"Key":"Rack_StudioEQ"},
                "Amp":{"Type":"Amps","KnobValues":{"Amp_AT20_Treble":40.0,"Amp_AT20_Gain":"34"},"Key":"Amp_AT20"},
                "Cabinet":{"Type":"Cabinets","KnobValues":{},"Key":"Cab_GB412CMKIII_Ribbon_Cone"},
                "PrePedal1":null},
               "ToneDescriptors":["$[35720]CLEAN"],"Volume":"-24.752081","Key":"1979_clean","Name":"1979_clean"}"#,
        )
        .unwrap();
        let t = RigTone::from_json(&v).unwrap();
        assert_eq!(t.key, "1979_clean");
        assert_eq!(t.descriptors, vec!["CLEAN"]);
        assert!((t.volume_db + 24.752).abs() < 1e-3);
        let slots: Vec<&str> = t.gear.iter().map(|g| g.slot.as_str()).collect();
        assert_eq!(slots, vec!["Amp", "Cabinet", "Rack1"]);
        assert_eq!(t.gear[0].knob("gain"), Some(34.0));
        assert_eq!(t.gear[2].knob("BassFreq"), Some(314.0));
        assert_eq!(t.label(), "clean");
    }

    #[test]
    fn labels_split_into_words() {
        let t = |k: &str| {
            RigTone {
                key: k.into(),
                ..Default::default()
            }
            .label()
        };
        assert_eq!(t("AudBe_wahsolo"), "wah solo");
        assert_eq!(t("AudBe_breakdowndrive"), "breakdown drive");
        assert_eq!(t("coldyell_cleanlead"), "clean lead");
        assert_eq!(t("croscold_leadlow"), "lead low");
        assert_eq!(t("BostonForeplay_12str"), "12str");
    }
}
