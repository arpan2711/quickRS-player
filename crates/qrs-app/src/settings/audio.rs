//! The Audio page, designed fresh on `qrs_audio::devices` (the Python app's page listed device names): the guitar
//! input and the output, each "Automatic" or a device you pick, with what each device is and whether it's
//! running; the guitar's channel, with a meter on each choice (found by itself from the first clear notes);
//! hearing the guitar, the latency and the buffer; the volumes; and the Rocksmith 2014 setup it starts from.
//! Every change is saved as config.json "audio" (`AudioPrefs`) and the device manager re-plans the route.
//! Laid out as the Python app's page (settingsview.py `draw_audio`), in its whole pixels.

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
use qrs_ui::widgets::{self, Segmented, segment_widths};

use super::{SettingsView, caption_lines, centre_x, centre_y, line_h, snap};
use crate::devstatus::same_device;
use crate::state::{AppState, AudioMode};

/// One choice in a device list.
pub(super) struct Row {
    label: String,
    note: String,
    choice: Choice,
}

/// Prefer the running stream's channel count, then the planned device's; assume stereo before either is known.
/// Keep at least one choice, without limiting selectable channels to the number of individual meters.
fn input_channels(audio: &Audio) -> usize {
    audio
        .engine
        .status()
        .input
        .as_ref()
        .map(|i| i.channels as usize)
        .or_else(|| {
            audio
                .plan()
                .input
                .as_ref()
                .map(|i| i.device.input_channels as usize)
        })
        .unwrap_or(2)
        .max(1)
}

/// Labels paired with AudioPrefs::channel: None for Auto, zero-based inputs, or -1 for all mixed.
/// A mono input needs only its single channel choice.
fn channel_options(n_ch: usize) -> Vec<(String, Option<i32>)> {
    if n_ch == 1 {
        return vec![("Channel 1".into(), Some(0))];
    }
    let mut opts = vec![("Auto".into(), None)];
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
    opts
}

/// Columns per row and total track height for a nonempty choice list, balancing rows to avoid a short last row.
/// Budget 80 points per choice for its label and meter, or use one column when the space is narrower.
fn channel_grid(n_choices: usize, w: f32) -> (usize, f32) {
    let max_columns = (w / px(80.0)).floor().max(1.0) as usize;
    let rows = n_choices.div_ceil(max_columns);
    let columns = n_choices.div_ceil(rows);
    let height = rows as f32 * px(76.0) + (rows - 1) as f32 * px(8.0);
    (columns, height)
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
    let rs = audio.rocksmith();
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
        label: "No guitar input".into(),
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
            (None, "No guitar input: nothing is scored".into())
        } else {
            (None, "Plug your guitar in: it's picked up by itself".into())
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

    /// Selector height including the gap and two-line hint, but not the card's heading or padding.
    /// Shared by Settings and the guide so their cards reserve room for the same wrapped choices.
    pub(super) fn h_channel(st: &AppState, ctx: &egui::Context, w: f32) -> f32 {
        let n_ch = st.audio.as_ref().map(input_channels).unwrap_or(2);
        let (_, height) = channel_grid(channel_options(n_ch).len(), w);
        height + px(14.0) + 2.0 * line_h(ctx, 13.0)
    }

    /// The guitar's channel: Auto, every reported input and all mixed, wrapping to fit. Individual meters cover
    /// the first 16 inputs. Returns the y under the hint.
    pub(super) fn draw_channel(&mut self, ui: &Ui, st: &AppState, x: f32, y: f32, w: f32) -> f32 {
        let Some(audio) = &st.audio else { return y };
        let p = ui.painter().clone();
        let prefs = audio.prefs();
        let plan = audio.plan();
        let status = audio.engine.status();
        let meters = audio.engine.meters();
        let silent = st.audio_mode == AudioMode::Silent;
        let n_ch = input_channels(audio);
        let opts = channel_options(n_ch);
        let sel = if n_ch == 1 {
            0
        } else {
            opts.iter().position(|o| o.1 == prefs.channel).unwrap_or(0)
        };
        let (columns, height) = channel_grid(opts.len(), w);
        let live = status.input.is_some() && !silent;
        let all_peak = meters.channels[..meters.channel_count.min(meters.channels.len())]
            .iter()
            .fold(0.0f32, |a, &b| a.max(b));
        for (row, choices) in opts.chunks(columns).enumerate() {
            let start = row * columns;
            let seg = Rect::from_min_size(pos2(x, y + row as f32 * (px(76.0) + px(8.0))), vec2(w, px(76.0)));
            let labels: Vec<&str> = choices.iter().map(|o| o.0.as_str()).collect();
            let mut so = Segmented::new(&labels, sel.checked_sub(start).filter(|&i| i < choices.len()));
            so.size = if opts.len() > 3 { 13.0 } else { 14.0 };
            so.gap = 6.0;
            so.extra = true;
            let (rects, clicked) = widgets::segmented(ui, seg, ("set-inch", row), &so);
            if let Some(i) = clicked
                && n_ch > 1
            {
                self.acts.push(match choices[i].1 {
                    None => "ch:auto".into(),
                    Some(c) => format!("ch:{c}"),
                });
            }
            for (rr, (_, ch_)) in rects.iter().zip(choices) {
                let pk = if !live {
                    0.0
                } else {
                    match ch_ {
                        None => meters.peak,
                        Some(-1) => all_peak,
                        Some(c) => meters.channels.get(*c as usize).copied().unwrap_or(0.0),
                    }
                };
                let mw =
                    (10.0 * px(5.0) + 9.0 * px(2.0).max(pixels(1.0))).min((rr.width() - px(8.0)).max(0.0));
                let mr = Rect::from_min_size(
                    pos2(centre_x(*rr) - snap(mw / 2.0), snap(rr.min.y + rr.height() * 0.6)),
                    vec2(mw, px(8.0)),
                );
                knobs::led_meter(&p, pk, mr, 10);
            }
        }
        let route_ch = plan.route.input.as_ref().map(|i| i.channel);
        let hint = match (route_ch, prefs.channel) {
            (None, _) => "Pick a guitar input above to set its channel.".to_string(),
            (Some(_), Some(_)) | (Some(_), None) if n_ch == 1 => {
                "Your input has one channel: that's the guitar.".into()
            }
            (Some(_), Some(_)) => {
                "Rocksmith-style cables and most interfaces put the guitar on channel 1.".into()
            }
            (Some(_), None) if !plan.channel_fixed => {
                "Pluck a string: the channel your guitar is on is found by itself.".into()
            }
            (Some(-1), None) => "Listening to every channel mixed.".into(),
            (Some(c), None) => format!("Your guitar is on channel {}: found by itself.", c + 1),
        };
        caption_lines(&p, &hint, x, y + height + px(14.0), w, 13.0, 2)
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
            self.cal_run = None;
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
        if let Some(c) = &out.cal {
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
    pub(super) fn draw_audio(&mut self, ui: &mut Ui, st: &mut AppState, body: Rect) -> f32 {
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
        let prefs = audio.prefs();
        let status = engine.status();
        let rocksmith = audio.rocksmith();
        let (inputs, in_cur) = input_rows(audio, &prefs);
        let (outputs, out_cur) = output_rows(audio, &prefs);
        let silent = st.audio_mode == AudioMode::Silent;
        let (gap, pad) = (px(24.0), px(20.0));
        let colw = snap((body.width() - gap) / 2.0);
        let hh = Self::h_heading(&ctx);
        let mut y = body.min.y;

        // the guided setup (P23.8): its way in, at the top
        y = self.draw_setup_entry(ui, st, body, y) + gap;

        // the devices (and a line under each list: what's running)
        let rows = inputs.len().max(outputs.len()).max(1) as f32;
        let list_h = 2.0 * px(8.0) + rows * px(46.0) + (rows - 1.0) * px(4.0);
        let ch = pad + hh + list_h + px(14.0) + line_h(&ctx, 13.0) + pad;
        let in_status = device_status(st, audio, true);
        let out_status = device_status(st, audio, false);
        for (c, (kind, title, note, rows_, cur, stat)) in [
            (
                "input",
                "GUITAR INPUT",
                "what your guitar plugs into",
                &inputs,
                in_cur,
                &in_status,
            ),
            (
                "output",
                "OUTPUT",
                "where you hear the song",
                &outputs,
                out_cur,
                &out_status,
            ),
        ]
        .into_iter()
        .enumerate()
        {
            let card = Rect::from_min_size(pos2(body.min.x + c as f32 * (colw + gap), y), vec2(colw, ch));
            paint::card(&p, card, 24.0, false);
            let yy = self.heading(
                &p,
                title,
                card.min.x + pad,
                card.min.y + pad,
                Some(card.max.x - pad),
                note,
                "",
            );
            let labels: Vec<String> = rows_.iter().map(|r| r.label.clone()).collect();
            let notes: Vec<String> = rows_.iter().map(|r| r.note.clone()).collect();
            // (neu.radio_rows' own row height and size: 46, 14)
            let (_, clicked) = widgets::radio_rows(
                ui,
                Rect::from_min_size(pos2(card.min.x + pad, yy), vec2(card.width() - 2.0 * pad, 0.0)),
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
            status_line(
                &p,
                card.min.x + pad + px(4.0),
                yy + list_h + px(14.0),
                card.width() - 2.0 * pad - px(4.0),
                stat.0,
                &stat.1,
            );
        }
        self.inputs = inputs.into_iter().map(|r| r.choice).collect();
        self.outputs = outputs.into_iter().map(|r| r.choice).collect();
        y += ch + gap;

        // the guitar's channel, and hearing the guitar (one more switch row than the Python app's: the buffer)
        let chan_h = 2.0 * pad + hh + Self::h_channel(st, &ctx, colw - 2.0 * pad);
        let hear_h = 2.0 * pad + hh + px(4.0) + 4.0 * Self::h_switch("lg", true) + 3.0 * px(12.0);
        let card_h = chan_h.max(hear_h);
        let card = Rect::from_min_size(pos2(body.min.x, y), vec2(colw, card_h));
        paint::card(&p, card, 24.0, false);
        let yy = self.heading(
            &p,
            "INPUT CHANNEL",
            card.min.x + pad,
            card.min.y + pad,
            Some(card.max.x - pad),
            "pick the one whose meter moves",
            "",
        );
        self.draw_channel(ui, st, card.min.x + pad, yy, card.width() - 2.0 * pad);

        let card = Rect::from_min_size(pos2(body.min.x + colw + gap, y), vec2(colw, card_h));
        paint::card(&p, card, 24.0, false);
        let mut yy = self.heading(
            &p,
            "HEARING YOUR GUITAR",
            card.min.x + pad,
            card.min.y + pad,
            None,
            "",
            "",
        );
        let ms =
            |i: &qrs_audio::backend::StreamInfo| (i.block as f64 / i.rate.max(1) as f64 + i.latency) * 1000.0;
        let lat = match (&status.input, &status.output) {
            _ if silent => "silent".to_string(),
            (Some(i), Some(o)) if status.duplex => {
                format!("in {:.1} ms + out {:.1} ms  ·  {}", ms(i), ms(o), o.mode)
            }
            (Some(i), Some(o)) => format!(
                "in {:.1} ms + out {:.1} ms  ·  {}",
                ms(i),
                ms(o),
                if i.mode == o.mode {
                    o.mode.clone()
                } else {
                    format!("{} / {}", i.mode, o.mode)
                }
            ),
            (None, Some(o)) => format!("out {:.1} ms  ·  {}", ms(o), o.mode),
            (Some(i), None) => format!("in {:.1} ms  ·  {}", ms(i), i.mode),
            (None, None) => "no devices".into(),
        };
        let lst = Style::new(12.0, 700, th.text_2);
        let lw = (text::width(&ctx, &lat, lst) + px(20.0)).min(snap(card.width() / 2.0));
        let chip = Rect::from_min_size(
            pos2(card.max.x - pad - lw, card.min.y + pad - px(6.0)),
            vec2(lw, px(28.0)),
        );
        paint::display(&p, chip, Some(9.0), 3.0);
        text::draw(
            &p,
            &lat,
            pos2(centre_x(chip), centre_y(chip)),
            Align2::CENTER_CENTER,
            lst,
            Some(lw - px(16.0)),
        );
        let (x, wd) = (card.min.x + pad, card.width() - 2.0 * pad);
        yy += px(4.0);
        let monitor = engine.monitor();
        yy += self.switch_row(
            ui,
            x,
            yy,
            wd,
            "Hear guitar",
            if st.mic_input() {
                "a microphone: off at first, it can feed back"
            } else {
                "play your guitar through the app"
            },
            monitor,
            "monitor",
            "lg",
            "M",
            700,
        ) + px(12.0);
        // Low-latency output, highlighted (owner request, 2026-10-29: mockup C): a glow, a tinted plate, a FAST badge
        // before the label, the hint in the highlight's colour (emerald in the dark themes, orange in Daylight)
        {
            let col = if qrs_ui::theme::is_dark() {
                egui::Color32::from_rgb(52, 199, 123)
            } else {
                egui::Color32::from_rgb(232, 112, 30)
            };
            let p = ui.painter().clone();
            let h = Self::h_switch("lg", true);
            let plate = Rect::from_min_size(
                pos2(x - px(12.0), yy - px(7.0)),
                vec2(wd + px(24.0), h + px(14.0)),
            );
            paint::glow(&p, plate, col, 8.0, Some(0.45), Some(px(14.0)));
            p.rect_filled(plate, px(14.0), t().base.lerp_to_gamma(col, 0.14));
            p.rect_stroke(
                plate,
                px(14.0),
                egui::Stroke::new(1.5, col),
                egui::StrokeKind::Inside,
            );
            let badge = Style::new(11.0, 800, egui::Color32::from_rgb(6, 34, 20));
            let bw = text::width(ui.ctx(), "FAST", badge) + px(16.0);
            let br = Rect::from_min_size(pos2(x, yy + h / 2.0 - px(9.0) - px(8.0)), vec2(bw, px(16.0)));
            p.rect_filled(br, px(8.0), col);
            text::draw(&p, "FAST", br.center(), Align2::CENTER_CENTER, badge, None);
            self.hint_color = Some(col);
            // (the label and its hint after the badge)
            yy += self
                .switch_row(
                    ui,
                    x + bw + px(8.0),
                    yy,
                    wd - bw - px(8.0),
                    "Low-latency output",
                    "quicker: skips the system's sound mixer \u{b7} other apps go quiet",
                    prefs.exclusive,
                    "exclusive",
                    "lg",
                    "",
                    700,
                )
                .max(h);
        }
        yy += px(12.0);
        yy += self.switch_row(
            ui,
            x,
            yy,
            wd,
            "Note widget",
            "the always-on-top note and key window",
            st.config.bool("note_widget", true),
            "notewidget",
            "lg",
            "",
            700,
        ) + px(12.0);
        // the buffer: smaller is quicker, bigger rides out a busy computer (a row like the switches', the choice
        // at its right as the switches are)
        let row = Rect::from_min_size(pos2(x, yy), vec2(wd, Self::h_switch("lg", true)));
        let cy = centre_y(row);
        let bufs: [(&str, Option<u32>); 5] = [
            ("Auto", None),
            ("64", Some(64)),
            ("128", Some(128)),
            ("256", Some(256)),
            ("512", Some(512)),
        ];
        let blabels: Vec<&str> = bufs.iter().map(|b| b.0).collect();
        let widths = segment_widths(&ctx, &blabels, 12.0, 2.0);
        // (as the tone page's: the widths, the gaps between them and the padding either side)
        let sw: f32 = widths.iter().sum::<f32>() + px(4.0) * (widths.len() - 1) as f32 + 2.0 * px(4.0);
        let bh = px(40.0);
        let bseg = Rect::from_min_size(pos2(row.max.x - sw, cy - snap(bh / 2.0)), vec2(sw, bh));
        let bsel = bufs.iter().position(|b| b.1 == prefs.buffer);
        let mut bo = Segmented::new(&blabels, bsel);
        bo.size = 12.0;
        bo.widths = Some(widths);
        bo.pad = 4.0;
        bo.r = 14.0;
        bo.inner_r = 10.0;
        let (_, bclicked) = widgets::segmented(ui, bseg, "set-buffer", &bo);
        if let Some(i) = bclicked {
            self.acts.push(match bufs[i].1 {
                None => "buffer:auto".into(),
                Some(n) => format!("buffer:{n}"),
            });
        }
        text::draw(
            &p,
            "Buffer",
            pos2(x, cy - px(9.0)),
            Align2::LEFT_CENTER,
            Style::new(15.0, 700, th.text),
            Some(bseg.min.x - x - px(12.0)),
        );
        text::draw(
            &p,
            "raise it if you hear crackles",
            pos2(x, cy + px(11.0)),
            Align2::LEFT_CENTER,
            Style::new(12.0, 500, th.caption),
            Some(bseg.min.x - x - px(12.0)),
        );
        y += card_h + gap;
        y = self.draw_input_level(ui, st, body, y) + gap;

        // the volumes
        let card = Rect::from_min_size(pos2(body.min.x, y), vec2(body.width(), px(104.0)));
        paint::card(&p, card, 24.0, false);
        let sw = snap((card.width() - 2.0 * px(24.0) - px(48.0)) / 2.0);
        let vol = engine.song_gain();
        let gvol = engine.guitar_gain();
        self.slider_row(
            ui,
            card.min.x + px(24.0),
            card.min.y + pad,
            sw,
            "Song volume",
            &format!("{}%", (vol * 100.0).round() as i64),
            vol,
            "vol",
            "9 0",
        );
        self.slider_row(
            ui,
            card.min.x + px(24.0) + sw + px(48.0),
            card.min.y + pad,
            sw,
            "Guitar volume",
            &format!("{}%", (gvol * 100.0).round() as i64),
            gvol / 2.0,
            "gvol",
            "7 8",
        );
        y = card.max.y + gap;

        // the Rocksmith 2014 setup
        let hs = Self::h_switch("md", true);
        let card = Rect::from_min_size(pos2(body.min.x, y), vec2(body.width(), pad + hh + hs + pad));
        paint::card(&p, card, 24.0, false);
        let (x, wd) = (card.min.x + pad, card.width() - 2.0 * pad);
        let note = match &rocksmith {
            Some(r) => format!("found in {}", r.folder.display()),
            None => "not found next to your songs or in Steam".into(),
        };
        let yy = self.heading(
            &p,
            "ROCKSMITH 2014",
            x,
            card.min.y + pad,
            Some(card.max.x - pad),
            &note,
            "",
        );
        let hint = match &rocksmith {
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
                    parts.push(format!("guitar channel {}", r.mono_channel + 1));
                }
                if r.exclusive_mode {
                    parts.push("exclusive mode".into());
                }
                parts.join("  ·  ")
            }
            None => "the devices and channel RS_ASIO and Rocksmith.ini use, when they're found".into(),
        };
        let bw = widgets::button_width(&ctx, "Look for devices", "", 14.0, false);
        self.switch_row(
            ui,
            x,
            yy,
            wd - bw - px(32.0),
            "Start on my Rocksmith setup",
            &hint,
            prefs.use_rocksmith,
            "rocksmith",
            "md",
            "",
            700,
        );
        // the key at the right, on the switch row's middle
        let bh = px(46.0);
        let br = Rect::from_min_size(
            pos2(card.max.x - pad - bw, yy + snap(hs / 2.0) - snap(bh / 2.0)),
            vec2(bw, bh),
        );
        self.button(
            ui,
            br,
            "Look for devices",
            "",
            "devrescan",
            widgets::ButtonStyle {
                size: 14.0,
                r: Some(14.0),
                ..Default::default()
            },
        );
        card.max.y
    }

    /// The way into the guided setup (P23.8): what it does, how this input stands, and Set up. Returns its bottom.
    fn draw_setup_entry(&mut self, ui: &Ui, st: &AppState, body: Rect, y: f32) -> f32 {
        let th = t();
        let p = ui.painter().clone();
        let ctx = ui.ctx().clone();
        let pad = px(20.0);
        let card = Rect::from_min_size(
            pos2(body.min.x, y),
            vec2(
                body.width(),
                2.0 * pad + line_h(&ctx, 17.0) + px(4.0) + line_h(&ctx, 13.0),
            ),
        );
        paint::card(&p, card, 24.0, false);
        let input = crate::calibrate::current_input(st);
        let cal = input
            .as_ref()
            .map(|(id, _)| crate::calibrate::load(&st.config, id))
            .filter(|c| c.peak_db.is_some());
        let label = if cal.is_some() {
            "Run it again"
        } else {
            "Set up your guitar"
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
        let tw = br.min.x - card.min.x - 2.0 * pad;
        let tr = text::draw(
            &p,
            "Guitar setup, step by step",
            pos2(card.min.x + pad, card.min.y + pad),
            Align2::LEFT_TOP,
            Style::new(17.0, 800, th.text),
            Some(tw),
        );
        let note = match (&input, &cal) {
            (Some((_, name)), Some(c)) => format!(
                "{name} is calibrated (peaks at {:.0} dB). The setup checks the input, its level and the notes.",
                c.peak_db.unwrap_or(0.0)
            ),
            _ => "About a minute: input and output, a level check, calibration and a note check.".into(),
        };
        text::draw(
            &p,
            &note,
            pos2(card.min.x + pad, tr.max.y + px(4.0)),
            Align2::LEFT_TOP,
            Style::new(13.0, 500, th.text_2),
            Some(tw),
        );
        card.max.y
    }

    /// The guitar input's level (D33): a meter of what comes in with the zone the pedalboard wants, Calibrate, the
    /// trim, and what kind of input it is (a dry guitar for the pedalboard, or an amp modeller heard as is). Each
    /// input keeps its own. Returns the card's bottom.
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
            .unwrap_or_default();
        let slider_h = line_h(&ctx, 15.0) + px(16.0) + px(12.0) + px(14.0);
        let card_h = 2.0 * pad + hh + px(58.0) + px(18.0) + slider_h + px(14.0) + 2.0 * line_h(&ctx, 13.0);
        let card = Rect::from_min_size(pos2(body.min.x, y), vec2(body.width(), card_h));
        paint::card(&p, card, 24.0, false);
        let (x, wd) = (card.min.x + pad, card.width() - 2.0 * pad);
        let note = match &input {
            Some((_, name)) => format!("{name}  ·  each input keeps its own"),
            None => "no guitar input yet".into(),
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
        let bw = widgets::button_width(&ctx, "Calibrate", "", 14.0, false).max(px(150.0));
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
        // Calibrate: its own two steps in the page's place (knobs up, strum hard; then the settings, P23.10)
        let br = Rect::from_min_size(pos2(card.max.x - pad - bw, yy), vec2(bw, px(46.0)));
        if input.is_some() {
            self.button(
                ui,
                br,
                "Calibrate",
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
        yy += px(58.0) + px(18.0);

        // the trim, and what kind of input this is
        let half = snap((wd - px(48.0)) / 2.0);
        let u = (cal.trim_db - calibrate::TRIM.0) / (calibrate::TRIM.1 - calibrate::TRIM.0);
        self.slider_row(
            ui,
            x,
            yy,
            half,
            "Trim",
            &format!("{:+.1} dB", cal.trim_db),
            u,
            "trim",
            "",
        );
        let kx = x + half + px(48.0);
        text::draw(
            &p,
            "This input is",
            pos2(kx, yy),
            Align2::LEFT_TOP,
            Style::new(15.0, 700, th.text),
            None,
        );
        let auto_label = match input.as_ref().map(|(_, name)| calibrate::auto_kind(st, name)) {
            Some(Kind::Mic) => "Auto: mic",
            Some(Kind::Modeller) => "Auto: amp",
            _ => "Auto: dry",
        };
        let labels = [auto_label, "Dry guitar", "Amp", "Mic"];
        let sel = match cal.kind {
            Kind::Auto => 0,
            Kind::Dry => 1,
            Kind::Modeller => 2,
            Kind::Mic => 3,
        };
        let seg = Rect::from_min_size(pos2(kx, yy + line_h(&ctx, 15.0) + px(8.0)), vec2(half, px(40.0)));
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
        yy += slider_h + px(14.0);

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
                format!("Calibrated: peaks at {pk:.0} dB, noise at {fl:.0} dB; {pluck}{own}"),
                th.text_2,
            )
        } else {
            (
                "Calibrate: turn the guitar's volume and tone all the way up, keep still for 2 seconds, then strum \
                 as hard as you can until the bar fills. Dry guitar inputs get the pedalboard; an amp or modeller \
                 is heard as it is."
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
        card.max.y
    }

    /// Calibrate, the trim and the input's kind (D33), saved per input in config.json `input_cal`.
    pub(super) fn cal_act(&mut self, st: &mut AppState, verb: &str, arg: &str) {
        use crate::calibrate::{self, Kind, Run};
        let Some((id, name)) = calibrate::current_input(st) else {
            self.say("No guitar input is listening");
            return;
        };
        let mut cal = calibrate::load(&st.config, &id);
        match (verb, arg) {
            ("cal", "start") => {
                let clips = st.audio.as_ref().map(|a| a.engine.meters().clips).unwrap_or(0);
                self.cal_run = Some(Run::new(id, clips, cal.kind));
                self.cal_out = None;
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
            ("calkind", k) => {
                cal.kind = Kind::parse(k);
                self.say(match cal.kind {
                    Kind::Dry => format!("{name}: a dry guitar, the pedalboard shapes it"),
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
                let list = if verb == "input" {
                    &self.inputs
                } else {
                    &self.outputs
                };
                let Some(choice) = arg.parse::<usize>().ok().and_then(|i| list.get(i)).cloned() else {
                    return;
                };
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
                    self.say(format!("Guitar input: {what}"));
                } else {
                    prefs.output = choice;
                    self.say(format!("Output: {what}"));
                }
            }
            "ch" => prefs.channel = arg.parse().ok(),
            "buffer" => prefs.buffer = arg.parse().ok(),
            "exclusive" => {
                prefs.exclusive = !prefs.exclusive;
                self.say(if prefs.exclusive {
                    "Low-latency output: other programs go quiet while quickRS plays"
                } else {
                    "Shared output: other programs can play too"
                });
            }
            "rocksmith" => prefs.use_rocksmith = !prefs.use_rocksmith,
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
    fn channel_options_include_every_input() {
        for n_ch in [4, 8, 16, 24] {
            let opts = channel_options(n_ch);
            assert_eq!(opts.len(), n_ch + 2);
            assert_eq!(opts[0], ("Auto".into(), None));
            for c in 0..n_ch {
                assert_eq!(opts[c + 1], (format!("In {}", c + 1), Some(c as i32)));
            }
            assert_eq!(opts[n_ch + 1], ("All".into(), Some(-1)));
        }
    }

    #[test]
    fn mono_and_stereo_channel_options_are_unchanged() {
        assert_eq!(channel_options(1), vec![("Channel 1".into(), Some(0))]);
        assert_eq!(
            channel_options(2),
            vec![
                ("Auto".into(), None),
                ("Channel 1".into(), Some(0)),
                ("Channel 2".into(), Some(1)),
                ("Both mixed".into(), Some(-1)),
            ]
        );
    }

    #[test]
    fn eight_channel_selector_wraps_without_squeezing_meters() {
        let opts = channel_options(8);
        let (columns, height) = channel_grid(opts.len(), px(480.0));
        assert_eq!(columns, 5);
        assert_eq!(height, px(160.0));
        let rows: Vec<_> = opts.chunks(columns).collect();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[1][3], ("In 8".into(), Some(7)));
        assert_eq!(rows[1][4], ("All".into(), Some(-1)));
    }

    #[test]
    fn channel_grid_adapts_to_width_and_keeps_small_devices_on_one_row() {
        for n_ch in [1, 2, 4] {
            let opts = channel_options(n_ch);
            assert_eq!(channel_grid(opts.len(), px(480.0)), (opts.len(), px(76.0)));
        }
        assert_eq!(channel_grid(10, px(800.0)), (10, px(76.0)));
        assert_eq!(channel_grid(10, px(320.0)), (4, px(244.0)));
        assert_eq!(channel_grid(10, 0.0), (1, px(832.0)));
    }
}
