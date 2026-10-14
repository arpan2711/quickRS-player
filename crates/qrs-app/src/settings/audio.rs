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
struct Row {
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
fn input_rows(audio: &Audio, prefs: &AudioPrefs) -> (Vec<Row>, Option<usize>) {
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
fn output_rows(audio: &Audio, prefs: &AudioPrefs) -> (Vec<Row>, Option<usize>) {
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
fn status_line(p: &egui::Painter, x: f32, y: f32, w: f32, light: Option<bool>, msg: &str) {
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
        let plan = audio.plan();
        let status = engine.status();
        let meters = engine.meters();
        let rocksmith = audio.rocksmith();
        let (inputs, in_cur) = input_rows(audio, &prefs);
        let (outputs, out_cur) = output_rows(audio, &prefs);
        let silent = st.audio_mode == AudioMode::Silent;
        let (gap, pad) = (px(24.0), px(20.0));
        let colw = snap((body.width() - gap) / 2.0);
        let hh = Self::h_heading(&ctx);
        let mut y = body.min.y;

        // the devices (and a line under each list: what's running)
        let rows = inputs.len().max(outputs.len()).max(1) as f32;
        let list_h = 2.0 * px(8.0) + rows * px(46.0) + (rows - 1.0) * px(4.0);
        let ch = pad + hh + list_h + px(14.0) + line_h(&ctx, 13.0) + pad;
        let in_status = if silent {
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
        } else if prefs.input == Choice::Off {
            (None, "No guitar input: nothing is scored".into())
        } else {
            (None, "Plug your guitar in: it's picked up by itself".into())
        };
        let out_status = if silent {
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
        };
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
        let chan_h = 2.0 * pad + hh + px(76.0) + px(14.0) + line_h(&ctx, 13.0);
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
        let seg = Rect::from_min_size(
            pos2(card.min.x + pad, yy),
            vec2(card.width() - 2.0 * pad, px(64.0) + 2.0 * px(6.0)),
        );
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
        caption_lines(
            &p,
            &hint,
            card.min.x + pad,
            seg.max.y + px(14.0),
            card.width() - 2.0 * pad,
            13.0,
            2,
        );

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
        yy += self.switch_row(
            ui,
            x,
            yy,
            wd,
            "Low-latency output",
            "WASAPI exclusive: other apps go quiet",
            prefs.exclusive,
            "exclusive",
            "lg",
            "",
            700,
        ) + px(12.0);
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
