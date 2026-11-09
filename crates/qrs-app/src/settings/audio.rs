//! The Sound page (P35.3-P35.6, D92; the Audio page before), designed fresh on `qrs_audio::devices` (the Python
//! app's page listed device names): the guided setup's status; [the instrument on this input, a slot for bass
//! mode]; the guitar input and the output, each the chosen device ("Automatic" or one you picked, whether it's
//! running) with Change… for the list; the guitar's channel, with a meter on each choice (found by itself from the
//! first clear notes); hearing yourself and the volumes; the input level (Check level, the kind of input, the trim
//! folded); folded under Advanced, the RS2014 setup it starts from and every device. Low-latency output and
//! the buffer are Timing's (timing.rs), the note widget Display's. Every change is saved as config.json "audio"
//! (`AudioPrefs`) and the device manager re-plans the route. Whole pixels, as the Python app's pages.

use eframe::egui;
use egui::{Align2, Rect, Ui, pos2, vec2};
use qrs_audio::backend::Backend;
use qrs_audio::devices::rank::{self, InputKind, RankedInput};
use qrs_audio::devices::{self, DevicePref};
use qrs_audio::{Audio, AudioPrefs, Choice};
use qrs_ui::knobs;
use qrs_ui::paint;
use qrs_ui::text::{self, Style};
use qrs_ui::theme::t;
use qrs_ui::units::{pixels, px};
use qrs_ui::widgets::{self, Segmented};

use super::{SettingsView, caption_lines, centre_x, centre_y, fastout, line_h, snap};
use crate::devstatus::same_device;
use crate::state::{AppState, AudioMode};

/// One choice in a device list.
pub(super) struct Row {
    label: String,
    note: String,
    choice: Choice,
}

fn kind_note(r: &RankedInput) -> &'static str {
    match r.kind {
        InputKind::RealToneCable => "Real Tone Cable",
        InputKind::Interface if r.device.id.backend == Backend::Asio => "ASIO driver",
        InputKind::Interface => "interface",
        InputKind::Microphone => "microphone",
        InputKind::SystemDefault => "follows the system",
        InputKind::Virtual => "virtual",
        _ => "",
    }
}

/// Automatic, the inputs best first (no test devices; virtual ones only when chosen), a chosen device that isn't
/// connected, and no input. Returns the rows and the current one.
pub(super) fn input_rows(audio: &Audio, prefs: &AudioPrefs) -> (Vec<Row>, Option<usize>) {
    let devices = audio.devices();
    let rs = audio.rs_setup();
    let auto = devices::plan(
        &AudioPrefs {
            input: Choice::Auto,
            ..prefs.clone()
        },
        &devices,
        rs.as_ref(),
    );
    let chosen = match &prefs.input {
        Choice::Device(p) => Some(p.clone()),
        _ => None,
    };
    let mut rows = vec![Row {
        label: "Automatic".into(),
        note: auto
            .input
            .map(|i| i.device.label())
            .unwrap_or_else(|| "nothing plugged in".into()),
        choice: Choice::Auto,
    }];
    for r in rank::rank_inputs(&devices, rs.as_ref())
        .into_iter()
        .filter(|r| r.kind != InputKind::Test)
        .filter(|r| {
            r.kind != InputKind::Virtual || chosen.as_ref().is_some_and(|p| same_device(p, &r.device))
        })
        .take(6)
    {
        rows.push(Row {
            label: r.device.label(),
            note: kind_note(&r).into(),
            choice: Choice::Device(DevicePref::of(&r.device)),
        });
    }
    let mut current = match &prefs.input {
        Choice::Auto => Some(0),
        _ => None,
    };
    if let Some(p) = &chosen {
        current = rows.iter().position(|row| match &row.choice {
            Choice::Device(d) => d.backend == p.backend && (d.id == p.id || d.name == p.name),
            _ => false,
        });
        if current.is_none() {
            rows.push(Row {
                label: p.name.clone(),
                note: "not connected".into(),
                choice: Choice::Device(p.clone()),
            });
            current = Some(rows.len() - 1);
        }
    }
    rows.push(Row {
        label: "No input".into(),
        note: String::new(),
        choice: Choice::Off,
    });
    if prefs.input == Choice::Off {
        current = Some(rows.len() - 1);
    }
    (rows, current)
}

/// Automatic (the system's default output, followed when it changes), then the outputs.
pub(super) fn output_rows(audio: &Audio, prefs: &AudioPrefs) -> (Vec<Row>, Option<usize>) {
    let outs = audio.outputs();
    let chosen = match &prefs.output {
        Choice::Device(p) => Some(p.clone()),
        _ => None,
    };
    let default = outs
        .iter()
        .find(|d| d.id.backend == Backend::Wasapi && d.is_default_output)
        .or_else(|| outs.iter().find(|d| d.id.backend == Backend::System));
    let mut rows = vec![Row {
        label: "Automatic".into(),
        note: default.map(|d| d.label()).unwrap_or_default(),
        choice: Choice::Auto,
    }];
    for d in outs
        .iter()
        .filter(|d| !matches!(d.id.backend, Backend::Null | Backend::System))
        .filter(|d| !rank::is_virtual(d) || chosen.as_ref().is_some_and(|p| same_device(p, d)))
        .take(6)
    {
        rows.push(Row {
            label: d.label(),
            note: if d.id.backend == Backend::Asio {
                "ASIO driver".into()
            } else if d.is_default_output {
                "default".into()
            } else {
                String::new()
            },
            choice: Choice::Device(DevicePref::of(d)),
        });
    }
    let mut current = match &prefs.output {
        Choice::Auto => Some(0),
        _ => None,
    };
    if let Some(p) = &chosen {
        current = rows.iter().position(|row| match &row.choice {
            Choice::Device(d) => d.backend == p.backend && (d.id == p.id || d.name == p.name),
            _ => false,
        });
        if current.is_none() {
            rows.push(Row {
                label: p.name.clone(),
                note: "not connected".into(),
                choice: Choice::Device(p.clone()),
            });
            current = Some(rows.len() - 1);
        }
    }
    (rows, current)
}

/// Sound's All devices fold (P35.6): every input (`input`) or output, virtual ones too, with the one chosen; the
/// input's list ends with No input. Returns the rows and the current one.
pub(super) fn rows_all(audio: &Audio, prefs: &AudioPrefs, input: bool) -> (Vec<Row>, Option<usize>) {
    let chosen = match if input { &prefs.input } else { &prefs.output } {
        Choice::Device(p) => Some(p.clone()),
        _ => None,
    };
    let mut rows = Vec::new();
    if input {
        let rs = audio.rs_setup();
        for r in rank::rank_inputs(&audio.devices(), rs.as_ref())
            .into_iter()
            .filter(|r| r.kind != InputKind::Test)
        {
            rows.push(Row {
                label: r.device.label(),
                note: kind_note(&r).into(),
                choice: Choice::Device(DevicePref::of(&r.device)),
            });
        }
    } else {
        for d in audio
            .outputs()
            .iter()
            .filter(|d| !matches!(d.id.backend, Backend::Null | Backend::System))
        {
            rows.push(Row {
                label: d.label(),
                note: if d.id.backend == Backend::Asio {
                    "ASIO driver".into()
                } else if rank::is_virtual(d) {
                    "virtual".into()
                } else {
                    String::new()
                },
                choice: Choice::Device(DevicePref::of(d)),
            });
        }
    }
    let mut current = chosen.as_ref().and_then(|p| {
        rows.iter().position(|row| match &row.choice {
            Choice::Device(d) => d.backend == p.backend && (d.id == p.id || d.name == p.name),
            _ => false,
        })
    });
    if let Some(p) = &chosen
        && current.is_none()
    {
        rows.push(Row {
            label: p.name.clone(),
            note: "not connected".into(),
            choice: Choice::Device(p.clone()),
        });
        current = Some(rows.len() - 1);
    }
    if input {
        rows.push(Row {
            label: "No input".into(),
            note: String::new(),
            choice: Choice::Off,
        });
        if prefs.input == Choice::Off {
            current = Some(rows.len() - 1);
        }
    }
    (rows, current)
}

/// A device's status under its name, without the name again: "Listening: NUX Audio (ASIO)  ·  ASIO  ·  2 ch" is
/// "Listening  ·  ASIO  ·  2 ch" (the rest as it is).
pub(super) fn short_status(msg: &str) -> String {
    for lead in ["Listening: ", "Playing on "] {
        if let Some(rest) = msg.strip_prefix(lead)
            && let Some((_, tail)) = rest.split_once("  \u{b7}  ")
        {
            return format!(
                "{}  \u{b7}  {tail}",
                lead.trim_end().trim_end_matches(':').trim_end_matches(" on")
            );
        }
    }
    msg.to_string()
}

/// One line of status under a device list: a light (running, failing, off) and what it says.
pub(super) fn status_line(p: &egui::Painter, x: f32, y: f32, w: f32, light: Option<bool>, msg: &str) {
    let th = t();
    let c = pos2(x + px(5.0), y + snap(line_h(p.ctx(), 13.0) / 2.0));
    match light {
        Some(true) => paint::led(p, c, 8.0, th.hit),
        Some(false) => paint::led(p, c, 8.0, th.miss),
        None => paint::dot(p, c, 4.0, th.unlit),
    }
    text::draw(
        p,
        msg,
        pos2(x + px(18.0), c.y),
        Align2::LEFT_CENTER,
        Style::new(13.0, 500, if light == Some(false) { th.miss } else { th.text_2 }),
        Some(w - px(18.0)),
    );
}

/// The line under the input's (`input`) or the output's device list: its light (running, failing, off) and words.
pub(super) fn device_status(st: &AppState, audio: &Audio, input: bool) -> (Option<bool>, String) {
    let silent = st.audio_mode == AudioMode::Silent;
    let status = audio.engine.status();
    let plan = audio.plan();
    if input {
        if silent {
            (None, "Silent mode: no sound device is opened".to_string())
        } else if let Some(err) = &status.input_error {
            (Some(false), err.clone())
        } else if let Some(n) = plan.notes.iter().find(|n| !n.contains("output")) {
            (Some(false), n.clone())
        } else if let Some(i) = &status.input {
            (
                Some(true),
                format!(
                    "Listening: {}  ·  {}  ·  {} ch  ·  {}",
                    i.device_name,
                    i.mode,
                    i.channels,
                    khz(i.rate)
                ),
            )
        } else if audio.prefs().input == Choice::Off {
            (None, "No input: nothing is scored".into())
        } else {
            (
                None,
                format!(
                    "Plug your {} in: it's picked up by itself",
                    st.instrument().noun()
                ),
            )
        }
    } else if silent {
        (None, "Silent mode: the song plays on a timer".to_string())
    } else if let Some(err) = &status.output_error {
        (Some(false), err.clone())
    } else if let Some(n) = plan.notes.iter().find(|n| n.contains("output")) {
        (Some(false), n.clone())
    } else if let Some(o) = &status.output
        && !status.silent_output
    {
        (
            Some(true),
            format!("Playing on {}  ·  {}  ·  {}", o.device_name, o.mode, khz(o.rate)),
        )
    } else {
        (None, "No sound device: the song plays silently".into())
    }
}

impl SettingsView {
    /// A device list as the Audio page draws it (the input's or the output's), at (x, y) and `w` wide, with rows
    /// `row_h` high; its rows are kept for the `input:i` / `output:i` acts. Returns the list's height.
    pub(super) fn device_list(
        &mut self,
        ui: &Ui,
        st: &AppState,
        input: bool,
        rect: (f32, f32, f32),
        row_h: f32,
    ) -> f32 {
        let Some(audio) = &st.audio else { return 0.0 };
        let prefs = audio.prefs();
        let (rows, cur) = if input {
            input_rows(audio, &prefs)
        } else {
            output_rows(audio, &prefs)
        };
        let kind = if input { "input" } else { "output" };
        let labels: Vec<String> = rows.iter().map(|r| r.label.clone()).collect();
        let notes: Vec<String> = rows.iter().map(|r| r.note.clone()).collect();
        let (x, y, w) = rect;
        let (_, clicked) = widgets::radio_rows(
            ui,
            Rect::from_min_size(pos2(x, y), vec2(w, 0.0)),
            ("set-dev", kind),
            &labels,
            &notes,
            cur,
            row_h,
            14.0,
        );
        if let Some(i) = clicked {
            self.acts.push(format!("{kind}:{i}"));
        }
        let n = rows.len() as f32;
        let choices: Vec<Choice> = rows.into_iter().map(|r| r.choice).collect();
        if input {
            self.inputs = choices;
        } else {
            self.outputs = choices;
        }
        2.0 * px(8.0) + n * px(row_h) + (n - 1.0).max(0.0) * px(4.0)
    }

    /// The guitar's channel: a choice for each (Auto, each channel, all mixed), a meter on each, and a line on how
    /// it's found. Returns the y under it.
    pub(super) fn draw_channel(&mut self, ui: &Ui, st: &AppState, x: f32, y: f32, w: f32) -> f32 {
        let Some(audio) = &st.audio else { return y };
        let p = ui.painter().clone();
        let prefs = audio.prefs();
        let plan = audio.plan();
        let status = audio.engine.status();
        let meters = audio.engine.meters();
        let silent = st.audio_mode == AudioMode::Silent;
        let n_ch = status
            .input
            .as_ref()
            .map(|i| i.channels as usize)
            .or_else(|| plan.input.as_ref().map(|i| i.device.input_channels as usize))
            .unwrap_or(2)
            .clamp(1, 4);
        // (label, prefs.channel) for each choice
        let mut opts: Vec<(String, Option<i32>)> = Vec::new();
        if n_ch == 1 {
            opts.push(("Channel 1".into(), Some(0)));
        } else {
            opts.push(("Auto".into(), None));
            for c in 0..n_ch {
                opts.push((
                    if n_ch == 2 {
                        format!("Channel {}", c + 1)
                    } else {
                        format!("In {}", c + 1)
                    },
                    Some(c as i32),
                ));
            }
            opts.push((if n_ch == 2 { "Both mixed" } else { "All" }.into(), Some(-1)));
        }
        let sel = if n_ch == 1 {
            0
        } else {
            opts.iter().position(|o| o.1 == prefs.channel).unwrap_or(0)
        };
        let seg = Rect::from_min_size(pos2(x, y), vec2(w, px(64.0) + 2.0 * px(6.0)));
        let labels: Vec<&str> = opts.iter().map(|o| o.0.as_str()).collect();
        let mut so = Segmented::new(&labels, Some(sel));
        so.size = if opts.len() > 3 { 13.0 } else { 14.0 };
        so.gap = 6.0;
        so.extra = true;
        let (rects, clicked) = widgets::segmented(ui, seg, "set-inch", &so);
        if let Some(i) = clicked
            && n_ch > 1
        {
            self.acts.push(match opts[i].1 {
                None => "ch:auto".into(),
                Some(c) => format!("ch:{c}"),
            });
        }
        let live = status.input.is_some() && !silent;
        let all_peak = meters.channels[..meters.channel_count.min(meters.channels.len())]
            .iter()
            .fold(0.0f32, |a, &b| a.max(b));
        for (rr, (_, ch_)) in rects.iter().zip(&opts) {
            let pk = if !live {
                0.0
            } else {
                match ch_ {
                    None => meters.peak,
                    Some(-1) => all_peak,
                    Some(c) => meters.channels.get(*c as usize).copied().unwrap_or(0.0),
                }
            };
            // mr.midtop = (rr.centerx, rr.y + rr.height * 0.6)
            let mw = 10.0 * px(5.0) + 9.0 * px(2.0).max(pixels(1.0));
            let mr = Rect::from_min_size(
                pos2(centre_x(*rr) - snap(mw / 2.0), snap(rr.min.y + rr.height() * 0.6)),
                vec2(mw, px(8.0)),
            );
            knobs::led_meter(&p, pk, mr, 10);
        }
        let route_ch = plan.route.input.as_ref().map(|i| i.channel);
        let n = st.instrument().noun();
        let hint = match (route_ch, prefs.channel) {
            (None, _) => "Pick an input above to set its channel.".to_string(),
            (Some(_), Some(_)) | (Some(_), None) if n_ch == 1 => {
                format!("Your input has one channel: that's the {n}.")
            }
            (Some(_), Some(_)) => {
                format!("RS-style cables and most interfaces put the {n} on channel 1.")
            }
            (Some(_), None) if !plan.channel_fixed => {
                format!("Pluck a string: the channel your {n} is on is found by itself.")
            }
            (Some(-1), None) => "Listening to every channel mixed.".into(),
            (Some(c), None) => format!("Your {n} is on channel {}: found by itself.", c + 1),
        };
        caption_lines(&p, &hint, x, seg.max.y + px(14.0), w, 13.0, 2)
    }
}

/// The input level meter in `groove`: -60..0 dBFS of what comes in (`raw`, the peak before the trim), the zone the
/// boards like (-18..-6), red near full scale, a tick where the trim takes it, and the scale under it. Returns where
/// a level (dBFS) is on it (x).
pub(super) fn level_meter(
    p: &egui::Painter,
    groove: Rect,
    raw: f32,
    trim_db: f32,
) -> impl Fn(f32) -> f32 + Copy {
    let th = t();
    let u_of = |db: f32| ((db + 60.0) / 60.0).clamp(0.0, 1.0);
    let at = move |db: f32| snap(groove.min.x + groove.width() * u_of(db));
    paint::inset(p, groove, Some(groove.height() / 2.0), 3.0, paint::Fill::Base);
    let zone = Rect::from_min_max(pos2(at(-18.0), groove.min.y), pos2(at(-6.0), groove.max.y));
    paint::wash(p, zone, th.hit, 0.18);
    let raw_db = 20.0 * raw.max(1e-7).log10();
    let inset = snap(groove.height() * 4.0 / 18.0);
    if raw > 1e-6 {
        let fill = Rect::from_min_max(
            groove.min + vec2(px(2.0), inset),
            pos2(at(raw_db).max(groove.min.x + px(12.0)), groove.max.y - inset),
        );
        let col = if raw_db > -1.0 {
            th.miss
        } else if raw_db > -4.0 {
            th.late
        } else {
            th.accent
        };
        p.rect_filled(fill, fill.height() / 2.0, col);
    }
    // where the trim takes it: a tick
    if raw > 1e-6 && trim_db.abs() >= 0.5 {
        let tx = at(raw_db + trim_db);
        p.rect_filled(
            Rect::from_min_size(
                pos2(tx - px(1.0), groove.min.y - px(4.0)),
                vec2(px(2.0), groove.height() + px(8.0)),
            ),
            1.0,
            th.text,
        );
    }
    for d in [-48.0f32, -36.0, -24.0, -12.0, 0.0] {
        text::draw(
            p,
            &format!("{d:.0}"),
            pos2(at(d), groove.max.y + px(6.0)),
            if d == 0.0 {
                Align2::RIGHT_TOP
            } else {
                Align2::CENTER_TOP
            },
            Style::new(11.0, 600, if d == -12.0 { th.text_2 } else { th.caption }),
            None,
        );
    }
    at
}

impl SettingsView {
    /// A calibration under way (D33, P23.10): fed this frame's meters (and, late in the quiet, a second of the
    /// recording to look for hum); dropped if the input changed; its outcome saved for the input and put on the
    /// engine when it's done, and returned then.
    pub(super) fn feed_calibration(
        &mut self,
        st: &mut AppState,
        ctx: &egui::Context,
    ) -> Option<crate::calibrate::Outcome> {
        use crate::calibrate;
        let run = self.cal_run.as_mut()?;
        if run.is_staged() {
            return None;
        }
        let input = calibrate::current_input(st);
        let engine = st.audio.as_ref().map(|a| a.engine.clone());
        ctx.request_repaint();
        if input.as_ref().map(|i| &i.0) != Some(&run.id) {
            // (unplugged, or another input picked, mid-run: P36.3)
            self.cal_run = None;
            self.cal_out = None;
            self.say("The input changed while calibrating: calibrate again");
            return None;
        }
        let e = engine?;
        if run.wants_hum() {
            // (the UI thread's own buffer: the audio thread only writes the ring)
            let cap = e.capture();
            let rate = cap.rate().max(8000);
            let mut buf = vec![0.0f32; (rate as usize).min(cap.capacity() / 2)];
            let share = if cap.latest(&mut buf).is_some() {
                calibrate::hum_share(&buf, rate)
            } else {
                0.0
            };
            run.set_hum(share);
        }
        let m = e.meters();
        let out = run.feed(m.level, m.peak, m.clips)?;
        let id = run.id.clone();
        self.cal_run = None;
        // (a run that asks for another go isn't saved: the last good calibration stays, or none; Keep these
        // settings anyway saves it, P36.3)
        if let Some(c) = out.cal.as_ref().filter(|_| !out.verdict.retry()) {
            calibrate::save(&mut st.config, &id, c);
            st.save_config();
            calibrate::apply(st);
        }
        self.say(out.message.clone());
        self.cal_out = Some(out.clone());
        Some(out)
    }
}

fn khz(rate: u32) -> String {
    let k = rate as f64 / 1000.0;
    if (k - k.round()).abs() < 0.05 {
        format!("{k:.0} kHz")
    } else {
        format!("{k:.1} kHz")
    }
}

impl SettingsView {
    pub(super) fn draw_sound(&mut self, ui: &mut Ui, st: &mut AppState, body: Rect) -> f32 {
        let th = t();
        let p = ui.painter().clone();
        let ctx = ui.ctx().clone();
        let Some(audio) = &st.audio else {
            let card = Rect::from_min_size(body.min, vec2(body.width(), px(150.0)));
            paint::card(&p, card, 24.0, false);
            paint::ring(&p, card, Some(24.0), 1.0, th.miss, false);
            text::draw(
                &p,
                "The audio engine isn't running",
                pos2(card.min.x + px(24.0), card.min.y + px(24.0)),
                Align2::LEFT_TOP,
                Style::new(18.0, 800, th.miss),
                None,
            );
            let lines = text::wrap(
                &ctx,
                "quickRS was started without sound, so there are no devices to set up.",
                Style::new(14.0, 500, th.text_2),
                card.width() - px(48.0),
            );
            for (i, line) in lines.iter().take(3).enumerate() {
                text::draw(
                    &p,
                    line,
                    pos2(card.min.x + px(24.0), card.min.y + px(60.0) + i as f32 * px(20.0)),
                    Align2::LEFT_TOP,
                    Style::new(14.0, 500, th.text_2),
                    None,
                );
            }
            return card.max.y;
        };
        let engine = audio.engine.clone();
        let gap = px(24.0);
        let mut y = body.min.y;

        // the guided setup (P23.8): how this input stands, and the way in
        y = self.draw_setup_entry(ui, st, body, y) + gap;
        // SLOT: the Instrument card (bass mode, P32)
        let after = self.draw_instrument_card(ui, st, body, y);
        if after > y {
            y = after + gap;
        }
        // the devices: each the chosen one, Change… for the list
        y = self.draw_devices(ui, st, body, y) + gap;
        // hearing yourself: the monitor and the volumes (also in a song's Controls, and on the keys)
        y = self.draw_hearing(ui, st, &engine, body, y) + gap;
        // the input level: the meter, Check level, what kind of input; the trim folded
        y = self.draw_input_level(ui, st, body, y) + gap;

        // advanced, folded: the RS2014 setup, every device
        text::caps(
            &p,
            "ADVANCED",
            pos2(body.min.x + px(4.0), y),
            12.0,
            th.caption,
            Align2::LEFT_TOP,
            None,
        );
        y += Self::h_heading(&ctx);
        y = self.draw_rs_fold(ui, st, body, y) + px(12.0);
        self.draw_all_devices_fold(ui, st, body, y)
    }

    /// The "Instrument on this input" card (bass mode, P32.1, D89): Guitar / Bass / 5-string bass, kept per input
    /// (`input_cal[id].instrument`; what the tuner's strings, the parts opened first and the notes heard follow). The
    /// Sound page's second card, under the guided setup. Returns the card's bottom.
    pub(super) fn draw_instrument_card(&mut self, ui: &mut Ui, st: &mut AppState, body: Rect, y: f32) -> f32 {
        use crate::calibrate;
        use qrs_analysis::instrument::Instrument;
        let p = ui.painter().clone();
        let ctx = ui.ctx().clone();
        let (pad, hh) = (px(20.0), Self::h_heading(&ctx));
        let input = calibrate::current_input(st);
        let cal = input
            .as_ref()
            .map(|(id, _)| calibrate::load(&st.config, id))
            .unwrap_or_else(|| calibrate::load(&st.config, ""));
        let seg_h = px(40.0);
        let card_h = 2.0 * pad + hh + seg_h.max(3.0 * line_h(&ctx, 13.0));
        let card = Rect::from_min_size(pos2(body.min.x, y), vec2(body.width(), card_h));
        paint::card(&p, card, 24.0, false);
        let (x, wd) = (card.min.x + pad, card.width() - 2.0 * pad);
        let note = match &input {
            Some((_, name)) => format!("{name}  \u{b7}  each input keeps its own"),
            None => "no input yet: the setup's pick".into(),
        };
        let yy = self.heading(
            &p,
            "INSTRUMENT",
            x,
            card.min.y + pad,
            Some(card.max.x - pad),
            &note,
            "",
        );
        let labels = ["Guitar", "Bass", "5-string bass"];
        let sel = Instrument::ALL.iter().position(|i| *i == cal.instrument);
        let seg_w = snap(wd * 0.42);
        let seg = Rect::from_min_size(pos2(x, yy), vec2(seg_w, seg_h));
        let mut so = Segmented::new(&labels, sel);
        so.size = 13.0;
        so.pad = 4.0;
        so.r = 14.0;
        so.inner_r = 10.0;
        let (_, clicked) = widgets::segmented(ui, seg, "set-calinst", &so);
        if let Some(i) = clicked {
            self.acts.push(format!("calinst:{}", Instrument::ALL[i].key()));
        }
        let words = match cal.instrument {
            Instrument::Bass5 => {
                "Notes are read as a bass's, down to the low B; bass parts open first, and the tuner has your five \
                 strings. Check its level as a bass."
            }
            Instrument::Bass => {
                "Notes are read as a bass's, down to the low E; bass parts open first, and the tuner has your four \
                 strings. Check its level as a bass."
            }
            Instrument::Guitar => {
                "Playing a bass? Pick Bass: its notes are read a bass's way and bass parts open first. A guitar can \
                 still play bass parts."
            }
        };
        let hx = seg.max.x + px(20.0);
        caption_lines(&p, words, hx, yy, x + wd - hx, 13.0, 3);
        card.max.y
    }

    /// GUITAR INPUT and OUTPUT side by side: each the chosen device (its light and what's running) with Change…,
    /// which opens the list under it; the input's channel with a meter each. Returns the cards' bottom.
    fn draw_devices(&mut self, ui: &mut Ui, st: &mut AppState, body: Rect, y: f32) -> f32 {
        let th = t();
        let p = ui.painter().clone();
        let ctx = ui.ctx().clone();
        let Some(audio) = &st.audio else { return y };
        let prefs = audio.prefs();
        let (gap, pad) = (px(24.0), px(20.0));
        let colw = snap((body.width() - gap) / 2.0);
        let hh = Self::h_heading(&ctx);
        let (inputs, in_cur) = input_rows(audio, &prefs);
        let (outputs, out_cur) = output_rows(audio, &prefs);
        let in_status = device_status(st, audio, true);
        let out_status = device_status(st, audio, false);
        let chosen_h = px(60.0);
        let list_h = |n: usize| 2.0 * px(8.0) + n as f32 * px(46.0) + (n as f32 - 1.0).max(0.0) * px(4.0);
        let in_open = self.folds.is_open("dev_input");
        let out_open = self.folds.is_open("dev_output");
        let chan_h = px(14.0) + line_h(&ctx, 14.0) + px(10.0) + px(76.0) + px(14.0) + line_h(&ctx, 13.0);
        let in_h = pad
            + hh
            + chosen_h
            + if in_open {
                px(12.0) + list_h(inputs.len())
            } else {
                0.0
            }
            + chan_h
            + pad;
        let out_h = pad
            + hh
            + chosen_h
            + if out_open {
                px(12.0) + list_h(outputs.len())
            } else {
                0.0
            }
            + px(14.0)
            + 2.0 * line_h(&ctx, 13.0)
            + pad;
        let card_h = in_h.max(out_h);
        let inst = st.instrument();
        let (in_title, in_note) = (
            inst.say("GUITAR INPUT"),
            format!("what your {} plugs into", inst.noun()),
        );
        for (c, (kind, title, note, rows_, cur, stat, open)) in [
            (
                "input",
                in_title.as_str(),
                in_note.as_str(),
                &inputs,
                in_cur,
                &in_status,
                in_open,
            ),
            (
                "output",
                "OUTPUT",
                "where you hear the song",
                &outputs,
                out_cur,
                &out_status,
                out_open,
            ),
        ]
        .into_iter()
        .enumerate()
        {
            let card = Rect::from_min_size(pos2(body.min.x + c as f32 * (colw + gap), y), vec2(colw, card_h));
            paint::card(&p, card, 24.0, false);
            let (x, wd) = (card.min.x + pad, card.width() - 2.0 * pad);
            let mut yy = self.heading(&p, title, x, card.min.y + pad, Some(card.max.x - pad), note, "");
            // the chosen device: its light, its name, what's running; Change… at the right
            let row = Rect::from_min_size(pos2(x, yy), vec2(wd, chosen_h));
            paint::inset(&p, row, Some(16.0), 3.0, paint::Fill::Base);
            let label = match cur.and_then(|i| rows_.get(i)) {
                Some(r) if r.choice == Choice::Auto && !r.note.is_empty() => format!("Automatic: {}", r.note),
                Some(r) => r.label.clone(),
                None => "Automatic".into(),
            };
            let blabel = if open { "Close" } else { "Change\u{2026}" };
            let bw = widgets::button_width(&ctx, blabel, "", 14.0, false).max(px(104.0));
            let br = Rect::from_min_size(
                pos2(row.max.x - px(8.0) - bw, centre_y(row) - px(21.0)),
                vec2(bw, px(42.0)),
            );
            self.button(
                ui,
                br,
                blabel,
                "",
                &format!("fold:dev_{kind}"),
                widgets::ButtonStyle {
                    size: 14.0,
                    r: Some(12.0),
                    active: open,
                    ..Default::default()
                },
            );
            let lc = pos2(row.min.x + px(18.0), centre_y(row));
            match stat.0 {
                Some(true) => paint::led(&p, lc, 8.0, th.hit),
                Some(false) => paint::led(&p, lc, 8.0, th.miss),
                None => paint::dot(&p, lc, 4.0, th.unlit),
            }
            let tw = br.min.x - row.min.x - px(46.0);
            text::draw(
                &p,
                &label,
                pos2(row.min.x + px(34.0), centre_y(row) - px(9.0)),
                Align2::LEFT_CENTER,
                Style::new(15.0, 800, th.text),
                Some(tw),
            );
            text::draw(
                &p,
                &short_status(&stat.1),
                pos2(row.min.x + px(34.0), centre_y(row) + px(11.0)),
                Align2::LEFT_CENTER,
                Style::new(
                    12.0,
                    500,
                    if stat.0 == Some(false) {
                        th.miss
                    } else {
                        th.caption
                    },
                ),
                Some(tw),
            );
            yy = row.max.y;
            if open {
                let labels: Vec<String> = rows_.iter().map(|r| r.label.clone()).collect();
                let notes: Vec<String> = rows_.iter().map(|r| r.note.clone()).collect();
                let (_, clicked) = widgets::radio_rows(
                    ui,
                    Rect::from_min_size(pos2(x, yy + px(12.0)), vec2(wd, 0.0)),
                    ("set-dev", kind),
                    &labels,
                    &notes,
                    cur,
                    46.0,
                    14.0,
                );
                if let Some(i) = clicked {
                    self.acts.push(format!("{kind}:{i}"));
                }
                yy += px(12.0) + list_h(rows_.len());
            }
            if kind == "input" {
                let ly = yy + px(14.0);
                text::draw(
                    &p,
                    "Channel",
                    pos2(x, ly),
                    Align2::LEFT_TOP,
                    Style::new(14.0, 700, th.text),
                    None,
                );
                text::draw(
                    &p,
                    "pick the one whose meter moves",
                    pos2(x + wd, ly + snap(line_h(&ctx, 14.0) / 2.0)),
                    Align2::RIGHT_CENTER,
                    Style::new(12.0, 500, th.caption),
                    Some(wd - px(90.0)),
                );
                self.draw_channel(ui, st, x, ly + line_h(&ctx, 14.0) + px(10.0), wd);
            } else {
                caption_lines(
                    &p,
                    "A device that comes back is picked again by itself; Automatic follows the system's default.",
                    x,
                    yy + px(14.0),
                    wd,
                    13.0,
                    2,
                );
            }
        }
        self.inputs = inputs.into_iter().map(|r| r.choice).collect();
        self.outputs = outputs.into_iter().map(|r| r.choice).collect();
        y + card_h
    }

    /// HEARING YOURSELF: Hear guitar (M) at the left, the song's and the guitar's volumes (9 0, 7 8) at the right.
    fn draw_hearing(
        &mut self,
        ui: &mut Ui,
        st: &mut AppState,
        engine: &std::sync::Arc<qrs_audio::Engine>,
        body: Rect,
        y: f32,
    ) -> f32 {
        let th = t();
        let p = ui.painter().clone();
        let ctx = ui.ctx().clone();
        let (gap, pad) = (px(48.0), px(22.0));
        let hh = Self::h_heading(&ctx);
        let slider_h = line_h(&ctx, 15.0) + px(16.0) + px(12.0) + px(14.0);
        let card = Rect::from_min_size(
            pos2(body.min.x, y),
            vec2(
                body.width(),
                2.0 * pad + (2.0 * slider_h + px(14.0)).max(hh + px(84.0)),
            ),
        );
        paint::card(&p, card, 24.0, false);
        let x = card.min.x + pad;
        let colw = snap((card.width() - 2.0 * pad - gap) / 2.0);
        let yy = self.heading(&p, "HEARING YOURSELF", x, card.min.y + pad, None, "", "");
        let monitor = engine.monitor();
        let h = self.switch_row(
            ui,
            x,
            yy,
            colw,
            &st.instrument().say("Hear guitar"),
            &if st.mic_input() {
                "a microphone: off at first, it can feed back".to_string()
            } else {
                st.instrument().say("play your guitar through quickRS")
            },
            monitor,
            "monitor",
            "lg",
            "M",
            700,
        );
        caption_lines(
            &p,
            "Also in a song's Controls, and on the keys.",
            x,
            yy + h + px(10.0),
            colw,
            12.0,
            2,
        );
        let vol = engine.song_gain();
        let gvol = engine.guitar_gain();
        let sx = x + colw + gap;
        self.slider_row(
            ui,
            sx,
            card.min.y + pad,
            colw,
            "Song volume",
            &format!("{}%", (vol * 100.0).round() as i64),
            vol,
            "vol",
            "9 0",
        );
        self.slider_row(
            ui,
            sx,
            card.min.y + pad + slider_h + px(14.0),
            colw,
            &st.instrument().say("Guitar volume"),
            &format!("{}%", (gvol * 100.0).round() as i64),
            gvol / 2.0,
            "gvol",
            "7 8",
        );
        let _ = th;
        card.max.y
    }

    /// The RS2014 setup, folded: what was found, and the switch to start from it.
    fn draw_rs_fold(&mut self, ui: &mut Ui, st: &mut AppState, body: Rect, y: f32) -> f32 {
        let Some(audio) = &st.audio else { return y };
        let prefs = audio.prefs();
        let rs_setup = audio.rs_setup();
        let setup = match &rs_setup {
            Some(r) => {
                let mut parts = Vec::new();
                if let Some(a) = r.rs_asio.as_ref().filter(|a| a.enable_asio) {
                    if let Some((drv, ch_)) = a.inputs.first() {
                        parts.push(format!("RS_ASIO: {drv}, channel {}", ch_ + 1));
                    }
                    if let Some(b) = a.custom_buffer.filter(|_| a.buffer_mode == "custom") {
                        parts.push(format!("buffer {b}"));
                    }
                } else {
                    parts.push(format!("input channel {}", r.mono_channel + 1));
                }
                if r.exclusive_mode {
                    parts.push("low-latency output".into());
                }
                parts.join("  \u{b7}  ")
            }
            None => "the devices and channel RS_ASIO and Rocksmith.ini use, when they're found".into(),
        };
        let hint = match &rs_setup {
            Some(r) => format!(
                "{}  \u{b7}  found in {}",
                if prefs.use_rs_setup {
                    "started from"
                } else {
                    "not used"
                },
                r.folder.display()
            ),
            None => "not found next to your songs or in Steam".into(),
        };
        let (h, open) = self.fold(ui, body.min.x, y, body.width(), "rs_setup", "RS2014 setup", &hint);
        if !prefs.use_rs_setup {
            Self::changed_dot(ui.painter(), pos2(body.max.x - px(18.0), y + snap(h / 2.0)));
        }
        let mut y = y + h;
        if open {
            let x = body.min.x + px(42.0);
            y += px(12.0);
            y += self.switch_row(
                ui,
                x,
                y,
                body.max.x - x - px(16.0),
                "Start on my RS setup",
                &setup,
                prefs.use_rs_setup,
                "rs_setup",
                "md",
                "",
                700,
            );
        }
        y
    }

    /// Every device, folded: Look for devices, and the whole lists (a device that isn't plugged in, a virtual one).
    fn draw_all_devices_fold(&mut self, ui: &mut Ui, st: &mut AppState, body: Rect, y: f32) -> f32 {
        let ctx = ui.ctx().clone();
        let (h, open) = self.fold(
            ui,
            body.min.x,
            y,
            body.width(),
            "devices",
            "All devices",
            "look for devices again; pick from the whole list, a virtual device among them",
        );
        let mut y = y + h;
        let Some(audio) = &st.audio else { return y };
        if !open {
            return y;
        }
        let prefs = audio.prefs();
        y += px(12.0);
        let x = body.min.x + px(42.0);
        let bw = widgets::button_width(&ctx, "Look for devices", "", 14.0, false);
        self.button(
            ui,
            Rect::from_min_size(pos2(x, y), vec2(bw, px(46.0))),
            "Look for devices",
            "",
            "devrescan",
            widgets::ButtonStyle {
                size: 14.0,
                r: Some(14.0),
                ..Default::default()
            },
        );
        // (an ASIO driver that crashed the helper when it was loaded, issue #1: said here, where you'd look for it)
        let left = qrs_audio::backend::asio_left_out();
        let (note, lines) = if left.is_empty() {
            ("after plugging something in that isn't listed".to_string(), 1)
        } else {
            (
                format!(
                    "Left out: the ASIO driver {} crashed when it was tried (its device may still be here without ASIO)",
                    left.iter()
                        .map(|n| format!("\"{n}\""))
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
                2,
            )
        };
        caption_lines(
            ui.painter(),
            &note,
            x + bw + px(16.0),
            y + px(if lines == 1 { 14.0 } else { 6.0 }),
            body.max.x - x - bw - px(16.0),
            12.0,
            lines,
        );
        y += px(46.0) + px(14.0);
        let gap = px(24.0);
        let colw = snap((body.max.x - x - gap) / 2.0);
        let (ins, in_cur) = rows_all(audio, &prefs, true);
        let (outs, out_cur) = rows_all(audio, &prefs, false);
        let mut bottom = y;
        for (c, (kind, rows_, cur)) in [("input", &ins, in_cur), ("output", &outs, out_cur)]
            .into_iter()
            .enumerate()
        {
            let labels: Vec<String> = rows_.iter().map(|r| r.label.clone()).collect();
            let notes: Vec<String> = rows_.iter().map(|r| r.note.clone()).collect();
            let (rects, clicked) = widgets::radio_rows(
                ui,
                Rect::from_min_size(pos2(x + c as f32 * (colw + gap), y), vec2(colw, 0.0)),
                ("set-devall", kind),
                &labels,
                &notes,
                cur,
                46.0,
                14.0,
            );
            if let Some(i) = clicked {
                self.acts.push(format!("{kind}:a{i}"));
            }
            if let Some(r) = rects.last() {
                bottom = bottom.max(r.max.y + px(8.0));
            }
        }
        self.inputs_all = ins.into_iter().map(|r| r.choice).collect();
        self.outputs_all = outs.into_iter().map(|r| r.choice).collect();
        bottom
    }

    /// The way into the guided setup (P23.8), slim: how this input stands, and Guided setup (accented while it's
    /// not been set up). Returns its bottom.
    fn draw_setup_entry(&mut self, ui: &Ui, st: &AppState, body: Rect, y: f32) -> f32 {
        let th = t();
        let p = ui.painter().clone();
        let ctx = ui.ctx().clone();
        let pad = px(20.0);
        let card = Rect::from_min_size(
            pos2(body.min.x, y),
            vec2(
                body.width(),
                2.0 * pad + line_h(&ctx, 16.0) + px(4.0) + line_h(&ctx, 12.0),
            ),
        );
        paint::card(&p, card, 24.0, false);
        let input = crate::calibrate::current_input(st);
        let cal = input
            .as_ref()
            .map(|(id, _)| crate::calibrate::load(&st.config, id))
            .filter(|c| c.peak_db.is_some());
        let label = if cal.is_some() {
            "Guided setup again"
        } else {
            "Guided setup"
        };
        let bw = widgets::button_width(&ctx, label, "", 14.0, false).max(px(190.0));
        let br = Rect::from_min_size(
            pos2(card.max.x - pad - bw, centre_y(card) - px(23.0)),
            vec2(bw, px(46.0)),
        );
        self.button(
            ui,
            br,
            label,
            "",
            "setup:open",
            widgets::ButtonStyle {
                size: 14.0,
                r: Some(14.0),
                primary: cal.is_none(),
                ..Default::default()
            },
        );
        let tx = card.min.x + pad + px(20.0);
        let tw = br.min.x - tx - pad;
        let (title, note) = match (&input, &cal) {
            (Some((_, name)), Some(c)) => (
                format!("{name} is set up"),
                match c.floor_db {
                    Some(f) => format!(
                        "peaks at {:.0} dB  \u{b7}  noise at {f:.0} dB  \u{b7}  the setup checks the input, its level \
                         and the notes",
                        c.peak_db.unwrap_or(0.0)
                    ),
                    None => format!("peaks at {:.0} dB", c.peak_db.unwrap_or(0.0)),
                },
            ),
            (Some((_, name)), None) => (
                format!("Set up {name}, step by step"),
                "About a minute: input and output, a level check, calibration and a short tune to play."
                    .to_string(),
            ),
            (None, _) => (
                format!("Set up your {}, step by step", st.instrument().noun()),
                "About a minute: input and output, a level check, calibration and a short tune to play."
                    .to_string(),
            ),
        };
        let tr = text::draw(
            &p,
            &title,
            pos2(tx, card.min.y + pad),
            Align2::LEFT_TOP,
            Style::new(16.0, 800, th.text),
            Some(tw),
        );
        let lc = pos2(card.min.x + pad + px(5.0), centre_y(tr));
        if cal.is_some() {
            paint::led(&p, lc, 8.0, th.hit);
        } else {
            paint::led(&p, lc, 8.0, th.accent);
        }
        text::draw(
            &p,
            &note,
            pos2(tx, tr.max.y + px(4.0)),
            Align2::LEFT_TOP,
            Style::new(12.0, 500, th.text_2),
            Some(tw),
        );
        card.max.y
    }

    /// The guitar input's level (D33): a meter of what comes in with the zone the pedalboard wants, Check level
    /// (the level tool), what kind of input it is (a dry guitar for the pedalboard, or an amp modeller heard as
    /// is), and the trim folded. Each input keeps its own. Returns the card's bottom.
    fn draw_input_level(&mut self, ui: &mut Ui, st: &mut AppState, body: Rect, y: f32) -> f32 {
        use crate::calibrate::{self, Kind};
        let th = t();
        let p = ui.painter().clone();
        let ctx = ui.ctx().clone();
        let (pad, hh) = (px(20.0), Self::h_heading(&ctx));
        let input = calibrate::current_input(st);
        let meters = st.audio.as_ref().map(|a| a.engine.meters());
        let _ = self.feed_calibration(st, &ctx);
        let cal = input
            .as_ref()
            .map(|(id, _)| calibrate::load(&st.config, id))
            .unwrap_or_else(|| calibrate::load(&st.config, "")); // (no input: the setup's instrument)
        let slider_h = line_h(&ctx, 15.0) + px(16.0) + px(12.0) + px(14.0);
        let trim_open = self.folds.is_open("trim");
        let kind_h = px(44.0);
        let card_h = 2.0 * pad
            + hh
            + px(58.0)
            + px(14.0)
            + kind_h
            + px(14.0)
            + 2.0 * line_h(&ctx, 13.0)
            + px(14.0)
            + px(widgets::FOLD_ROW_H)
            + if trim_open { px(14.0) + slider_h } else { 0.0 };
        let card = Rect::from_min_size(pos2(body.min.x, y), vec2(body.width(), card_h));
        paint::card(&p, card, 24.0, false);
        let (x, wd) = (card.min.x + pad, card.width() - 2.0 * pad);
        let note = match &input {
            Some((_, name)) => format!("{name}  \u{b7}  each input keeps its own"),
            None => "no input yet".into(),
        };
        let mut yy = self.heading(
            &p,
            "INPUT LEVEL",
            x,
            card.min.y + pad,
            Some(card.max.x - pad),
            &note,
            "",
        );

        // the meter: -60..0 dBFS of what comes in (before the trim), the zone the boards like, the trim's result
        let bw = widgets::button_width(&ctx, "Check level", "", 14.0, false).max(px(150.0));
        let mw = snap(wd - bw - px(32.0));
        let groove = Rect::from_min_size(pos2(x, yy + px(8.0)), vec2(mw, px(18.0)));
        let raw = meters.as_ref().map(|m| m.peak).unwrap_or(0.0);
        let at = level_meter(&p, groove, raw, cal.trim_db);
        let raw_db = 20.0 * raw.max(1e-7).log10();
        let after_db = raw_db + cal.trim_db;
        let live = match &input {
            Some(_) if raw > 1e-6 => {
                if cal.trim_db.abs() >= 0.5 {
                    format!("now {raw_db:.0} dB  ·  {after_db:.0} dB into the tone")
                } else {
                    format!("now {raw_db:.0} dB")
                }
            }
            Some(_) => "play a note to see it".into(),
            None => String::new(),
        };
        text::draw(
            &p,
            &live,
            pos2(groove.min.x, groove.max.y + px(6.0)),
            Align2::LEFT_TOP,
            Style::new(11.0, 600, th.caption),
            Some(at(-48.0) - groove.min.x - px(16.0)),
        );
        // Check level: the level tool's two steps in the page's place (knobs up, strum hard; then the settings)
        let br = Rect::from_min_size(pos2(card.max.x - pad - bw, yy), vec2(bw, px(46.0)));
        if input.is_some() {
            self.button(
                ui,
                br,
                "Check level",
                "",
                "setup:calibrate",
                widgets::ButtonStyle {
                    size: 14.0,
                    r: Some(14.0),
                    primary: cal.peak_db.is_none(),
                    ..Default::default()
                },
            );
        }
        yy += px(58.0) + px(14.0);

        // what kind of input this is, and what that means
        let lr = text::draw(
            &p,
            "This input is",
            pos2(x, yy + snap(kind_h / 2.0)),
            Align2::LEFT_CENTER,
            Style::new(15.0, 700, th.text),
            None,
        );
        let auto_label = match input.as_ref().map(|(_, name)| calibrate::auto_kind(st, name)) {
            Some(Kind::Mic) => "Auto: mic",
            Some(Kind::Modeller) => "Auto: amp",
            _ => "Auto: dry",
        };
        let dry = format!("Dry {}", st.instrument().noun());
        let labels = [auto_label, dry.as_str(), "Amp / modeller", "Mic"];
        let sel = match cal.kind {
            Kind::Auto => 0,
            Kind::Dry => 1,
            Kind::Modeller => 2,
            Kind::Mic => 3,
        };
        let sx = lr.max.x + px(18.0);
        let seg_w = snap((wd * 0.58).min(x + wd - sx - px(180.0)));
        let seg = Rect::from_min_size(pos2(sx, yy), vec2(seg_w, kind_h));
        let mut so = Segmented::new(&labels, Some(sel));
        so.size = 13.0;
        so.pad = 4.0;
        so.r = 14.0;
        so.inner_r = 10.0;
        let (_, clicked) = widgets::segmented(ui, seg, "set-calkind", &so);
        if let Some(i) = clicked {
            self.acts.push(format!(
                "calkind:{}",
                [Kind::Auto, Kind::Dry, Kind::Modeller, Kind::Mic][i].key()
            ));
        }
        let hx = seg.max.x + px(16.0);
        caption_lines(
            &p,
            "Dry: quickRS's tone plays it. Amp / modeller: heard as it is (the tone As is).",
            hx,
            yy + px(2.0),
            x + wd - hx,
            12.0,
            3,
        );
        yy += kind_h + px(14.0);

        // what's happening, or the last result, or how to start
        let pluck = format!(
            "plucks count from {:.0} dB",
            20.0 * cal.onset_or_default(st.mic_input()).log10()
        );
        let own = if cal.is_recommended() {
            ""
        } else {
            " (set by hand)"
        };
        let (msg, col) = if let Some(o) = &self.cal_out {
            (
                format!("{}; {pluck}", o.message),
                if o.ok { th.hit } else { th.late },
            )
        } else if let (Some(pk), Some(fl)) = (cal.peak_db, cal.floor_db) {
            (
                format!("Level checked: peaks at {pk:.0} dB, noise at {fl:.0} dB; {pluck}{own}"),
                th.text_2,
            )
        } else if cal.instrument.is_bass() {
            (
                "Calibrate: turn the bass's volume all the way up, keep still for 2 seconds, then pluck the open \
                 strings as hard as you can until the bar fills. A dry input gets the rig; an amp or \
                 modeller is heard as it is."
                    .into(),
                th.caption,
            )
        } else {
            (
                "Check level: turn the guitar's volume and tone all the way up, keep still for 2 seconds, then strum \
                 as hard as you can until the bar fills. A dry input gets the rig; an amp or modeller is \
                 heard as it is."
                    .into(),
                th.caption,
            )
        };
        let lines = text::wrap(&ctx, &msg, Style::new(13.0, 600, col), wd);
        for (i, line) in lines.iter().take(2).enumerate() {
            text::draw(
                &p,
                line,
                pos2(x, yy + i as f32 * line_h(&ctx, 13.0)),
                Align2::LEFT_TOP,
                Style::new(13.0, 600, col),
                None,
            );
        }
        yy += 2.0 * line_h(&ctx, 13.0) + px(14.0);

        // the trim, folded: Check level sets it
        let (h, open) = self.fold(
            ui,
            x,
            yy,
            wd,
            "trim",
            "Input boost by hand",
            &format!(
                "{:+.1} dB  \u{b7}  Check level sets it; change it only if the meter stays out of the green zone",
                cal.trim_db
            ),
        );
        if cal.has_recommendation() && !cal.is_recommended() {
            Self::changed_dot(&p, pos2(x + wd - px(18.0), yy + snap(h / 2.0)));
        }
        yy += h;
        if open {
            yy += px(14.0);
            let u = (cal.trim_db - calibrate::TRIM.0) / (calibrate::TRIM.1 - calibrate::TRIM.0);
            let tx = x + px(42.0);
            let half = snap((wd - px(42.0) - px(48.0)) / 2.0);
            self.slider_row(
                ui,
                tx,
                yy,
                half,
                "Input boost",
                &format!("{:+.1} dB", cal.trim_db),
                u,
                "trim",
                "",
            );
            if cal.has_recommendation() {
                let label = "Back to what Check level found";
                let bw = widgets::button_width(&ctx, label, "", 13.0, false);
                self.button(
                    ui,
                    Rect::from_min_size(pos2(tx + half + px(48.0), yy + px(8.0)), vec2(bw, px(40.0))),
                    label,
                    "",
                    "calrec",
                    widgets::ButtonStyle {
                        size: 13.0,
                        r: Some(12.0),
                        enabled: !cal.is_recommended(),
                        ..Default::default()
                    },
                );
            }
        }
        card.max.y
    }

    /// Calibrate, the trim and the input's kind (D33), saved per input in config.json `input_cal`.
    pub(super) fn cal_act(&mut self, st: &mut AppState, verb: &str, arg: &str) {
        use crate::calibrate::{self, Kind, Run};
        let Some((id, name)) = calibrate::current_input(st) else {
            self.say("No input is listening");
            return;
        };
        let mut cal = calibrate::load(&st.config, &id);
        match (verb, arg) {
            ("cal", "start") => {
                let clips = st.audio.as_ref().map(|a| a.engine.meters().clips).unwrap_or(0);
                let mut run = Run::for_instrument(id, clips, cal.kind, cal.instrument);
                run.device = match cal.kind {
                    calibrate::Kind::Auto => calibrate::auto_kind(st, &name),
                    k => k,
                };
                self.cal_run = Some(run);
                self.cal_out = None;
                return;
            }
            ("cal", "keep") => {
                // (a verdict that asked for another go, kept anyway: P36.3, the hardware can't always be turned
                // down; it counts as calibrated, the verdict still shown)
                let Some(mut c) = self.cal_out.as_ref().and_then(|o| o.cal.clone()) else {
                    return;
                };
                c.kept = true;
                c.instrument = cal.instrument;
                calibrate::save(&mut st.config, &id, &c);
                st.save_config();
                calibrate::apply(st);
                if let Some(o) = &mut self.cal_out {
                    o.cal = Some(c);
                }
                self.say(format!("Kept for {name}: trim and pluck threshold as measured"));
                return;
            }
            ("cal", "cancel") => {
                self.cal_run = None;
                return;
            }
            ("settrim", u) => {
                let u: f32 = u.parse().unwrap_or(0.0);
                let db = calibrate::TRIM.0 + u.clamp(0.0, 1.0) * (calibrate::TRIM.1 - calibrate::TRIM.0);
                cal.trim_db = (db * 2.0).round() / 2.0;
                calibrate::save(&mut st.config, &id, &cal);
                calibrate::apply(st);
                return; // (saved to disk when the drag ends)
            }
            ("setpluck", u) => {
                cal.onset = Some(calibrate::onset_of(u.parse().unwrap_or(0.5)));
                calibrate::save(&mut st.config, &id, &cal);
                calibrate::apply(st);
                return; // (saved to disk when the drag ends)
            }
            // the keys: the trim in half-dB steps, the threshold a dB at a time
            ("trimstep", d) => {
                let d: f32 = d.parse().unwrap_or(0.0);
                cal.trim_db = (cal.trim_db + d).clamp(calibrate::TRIM.0, calibrate::TRIM.1);
            }
            ("pluckstep", d) => {
                let d: f32 = d.parse().unwrap_or(0.0);
                let o = calibrate::db(cal.onset_or_default(st.mic_input())) + d;
                cal.onset = Some(calibrate::lin(o).clamp(calibrate::ONSET.0, calibrate::ONSET.1));
            }
            ("calrec", _) => {
                if !cal.has_recommendation() {
                    return;
                }
                cal.restore_recommended();
                self.say(format!("{name}: back to the recommended settings"));
            }
            ("calinst", k) => {
                let was = cal.instrument;
                cal.instrument = qrs_analysis::instrument::Instrument::parse(k);
                if cal.instrument == was {
                    return;
                }
                self.say(match cal.instrument {
                    qrs_analysis::instrument::Instrument::Guitar => format!("{name}: a guitar"),
                    i => format!(
                        "{name}: a {}, its notes read as a bass's; calibrate it as one",
                        i.label().to_lowercase()
                    ),
                });
            }
            ("calkind", k) => {
                cal.kind = Kind::parse(k);
                self.say(match cal.kind {
                    Kind::Dry => format!("{name}: straight in, the rig shapes it"),
                    Kind::Modeller => format!("{name}: an amp or modeller, heard as it is"),
                    Kind::Mic => format!("{name}: a microphone, heard as it is"),
                    Kind::Auto => format!("{name}: told apart by its name"),
                });
            }
            _ => return,
        }
        calibrate::save(&mut st.config, &id, &cal);
        st.save_config();
        calibrate::apply(st);
    }

    /// Device and route changes: saved as config.json "audio", and the device manager re-plans the route.
    pub(super) fn audio_act(&mut self, st: &mut AppState, verb: &str, arg: &str) {
        // the Reduce delay card's (shared with the song's Controls panel, which calls `fastout::apply` too)
        let whole = if arg.is_empty() {
            verb.to_string()
        } else {
            format!("{verb}:{arg}")
        };
        if let Some(act) = fastout::Act::parse(&whole) {
            match fastout::apply(st, act) {
                Ok(m) | Err(m) => self.say(m),
            }
            return;
        }
        if st.audio_mode == AudioMode::Silent {
            self.say("Silent mode: quickRS isn't using any sound device");
            return;
        }
        let Some(audio) = &st.audio else {
            self.say("The audio engine isn't running");
            return;
        };
        let mut prefs = audio.prefs();
        match verb {
            "input" | "output" => {
                // "a<i>": a row of All devices' lists; "<i>": of the list under Change… (which closes again)
                let (all, i) = match arg.strip_prefix('a') {
                    Some(i) => (true, i),
                    None => (false, arg),
                };
                let list = match (verb == "input", all) {
                    (true, false) => &self.inputs,
                    (false, false) => &self.outputs,
                    (true, true) => &self.inputs_all,
                    (false, true) => &self.outputs_all,
                };
                let Some(choice) = i.parse::<usize>().ok().and_then(|i| list.get(i)).cloned() else {
                    return;
                };
                self.folds.close(if verb == "input" {
                    "dev_input"
                } else {
                    "dev_output"
                });
                let what = match &choice {
                    Choice::Auto => "automatic".to_string(),
                    Choice::Off => "none".into(),
                    Choice::Device(d) => d.name.clone(),
                };
                if verb == "input" {
                    if prefs.input != choice {
                        prefs.channel = None; // a new device: find its channel again
                    }
                    prefs.input = choice;
                    self.say(format!("{} input: {what}", st.instrument().noun_cap()));
                } else {
                    prefs.output = choice;
                    self.say(format!("Output: {what}"));
                }
            }
            "ch" => prefs.channel = arg.parse().ok(),
            "rs_setup" => prefs.use_rs_setup = !prefs.use_rs_setup,
            "devrescan" => {
                audio.rescan();
                self.say("Looking for sound devices…");
                return;
            }
            _ => return,
        }
        audio.set_prefs(prefs.clone());
        st.config
            .set("audio", serde_json::to_value(&prefs).unwrap_or_default());
        st.save_config();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_chosen_device_s_status_leaves_its_name_out() {
        assert_eq!(
            short_status("Listening: NUX Audio (ASIO)  \u{b7}  ASIO  \u{b7}  2 ch  \u{b7}  48 kHz"),
            "Listening  \u{b7}  ASIO  \u{b7}  2 ch  \u{b7}  48 kHz"
        );
        assert_eq!(
            short_status("Playing on Speakers  \u{b7}  WASAPI shared  \u{b7}  48 kHz"),
            "Playing  \u{b7}  WASAPI shared  \u{b7}  48 kHz"
        );
        assert_eq!(
            short_status("No input: nothing is scored"),
            "No input: nothing is scored"
        );
    }
}
