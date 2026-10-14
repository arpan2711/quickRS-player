//! What the app says about the sound devices (P6.11, new; the Python app had no hot-plug): the guitar or the
//! output going and coming back, the route switching by itself (an interface plugged in, the default output
//! changed), the guitar's channel found from the signal, and an offer to switch to a guitar input that was just
//! plugged in when you had picked another device (on Automatic the device manager takes it by itself).

use std::time::{Duration, Instant};

use eframe::egui;
use egui::{Align2, Rect, Ui, pos2, vec2};
use qrs_audio::backend::{DeviceId, DeviceInfo};
use qrs_audio::devices::DevicePref;
use qrs_audio::devices::rank::InputKind;
use qrs_audio::{Audio, AudioEvent, Choice, Event};
use qrs_ui::paint;
use qrs_ui::text::{self, Style};
use qrs_ui::theme::t;
use qrs_ui::widgets::{self, ButtonStyle};

use crate::state::{AppState, AudioMode};

/// How long the offer stays up.
const OFFER_FOR: Duration = Duration::from_secs(30);
const NOTICE_FOR: Duration = Duration::from_secs(6);

/// A device you chose, matched against one that's there (by id, else by name).
pub fn same_device(p: &DevicePref, d: &DeviceInfo) -> bool {
    p.backend == d.id.backend && (p.id == d.id.id || p.name == d.name)
}

struct Offer {
    pref: DevicePref,
    id: DeviceId,
    label: String,
    what: String,
    since: Instant,
}

#[derive(Default)]
pub struct DevStatus {
    /// The inputs seen so far (None until the first look).
    known: Option<Vec<DeviceId>>,
    offer: Option<Offer>,
    /// Offers turned down this session.
    dismissed: Vec<DeviceId>,
    /// A notice for pages that have no toast of their own (Home).
    notice: Option<(String, String, Instant)>,
}

/// The first part of an error, short enough for a toast.
fn short(m: &str) -> String {
    let m = m.trim();
    if m.chars().count() > 70 {
        let s: String = m.chars().take(68).collect();
        format!("{}…", s.trim_end())
    } else {
        m.to_string()
    }
}

impl DevStatus {
    /// Take in what the device manager reported; returns the notices to show, as (message, tail).
    pub fn poll(&mut self, st: &AppState) -> Vec<(String, String)> {
        let Some(audio) = &st.audio else {
            return Vec::new();
        };
        if matches!(st.audio_mode, AudioMode::Silent | AudioMode::Shots) {
            let _ = audio.events();
            return Vec::new();
        }
        if self.known.is_none() {
            self.known = Some(audio.ranked_inputs().into_iter().map(|r| r.device.id).collect());
        }
        let mut out = Vec::new();
        for ev in audio.events() {
            if matches!(
                ev,
                AudioEvent::Engine(Event::InputLost(_) | Event::OutputLost(_)) | AudioEvent::Switched { .. }
            ) {
                crate::applog::write(&format!("sound devices: {ev:?}"));
            }
            match ev {
                AudioEvent::Engine(Event::InputLost(m)) => out.push((
                    "Guitar input lost".into(),
                    format!("  ·  {}  ·  reconnecting…", short(&m)),
                )),
                AudioEvent::Engine(Event::InputBack(n)) => {
                    out.push((format!("Guitar back: {n}"), String::new()))
                }
                AudioEvent::Engine(Event::OutputLost(m)) => out.push((
                    "Sound device lost".into(),
                    format!("  ·  {}  ·  the song keeps time", short(&m)),
                )),
                AudioEvent::Engine(Event::OutputBack(n)) => {
                    out.push((format!("Sound back: {n}"), String::new()))
                }
                AudioEvent::Engine(Event::Changed) => {}
                AudioEvent::Switched { input, output } => out.push((
                    match input {
                        Some(i) => format!("Guitar input: {i}"),
                        None => "No guitar input".into(),
                    },
                    output.map(|o| format!("  ·  sound: {o}")).unwrap_or_default(),
                )),
                AudioEvent::ChannelFound(c) => out.push((
                    if c < 0 {
                        "Listening to every input channel".into()
                    } else {
                        format!("Found your guitar on channel {}", c + 1)
                    },
                    String::new(),
                )),
                AudioEvent::DevicesChanged => self.devices_changed(audio),
            }
        }
        out
    }

    /// Something was plugged in or out: offer a new guitar input if you'd picked another device.
    fn devices_changed(&mut self, audio: &Audio) {
        let ranked = audio.ranked_inputs();
        let before = self.known.take().unwrap_or_default();
        self.known = Some(ranked.iter().map(|r| r.device.id.clone()).collect());
        // an offer turned down can come back the next time the device is plugged in
        self.dismissed
            .retain(|d| ranked.iter().any(|r| r.device.id == *d));
        if let Some(o) = &self.offer
            && !ranked.iter().any(|r| r.device.id == o.id)
        {
            self.offer = None;
        }
        let Choice::Device(chosen) = audio.prefs().input else {
            return; // Automatic (or no input): nothing to offer
        };
        if !ranked.iter().any(|r| same_device(&chosen, &r.device)) {
            return; // the device you chose isn't there: the plan already uses the best one meanwhile
        }
        let fresh = ranked.iter().find(|r| {
            !before.contains(&r.device.id)
                && matches!(r.kind, InputKind::RealToneCable | InputKind::Interface)
                && !self.dismissed.contains(&r.device.id)
                && !same_device(&chosen, &r.device)
        });
        if let Some(r) = fresh {
            self.offer = Some(Offer {
                pref: DevicePref::of(&r.device),
                id: r.device.id.clone(),
                label: r.device.label(),
                what: r.reason.clone(),
                since: Instant::now(),
            });
        }
    }

    /// Screenshot mode: an offer and a notice as they'd look when a guitar is plugged in.
    pub fn stage(&mut self, label: &str, what: &str) {
        self.offer = Some(Offer {
            pref: DevicePref {
                backend: qrs_audio::backend::Backend::Asio,
                id: label.into(),
                name: label.into(),
            },
            id: DeviceId {
                backend: qrs_audio::backend::Backend::Asio,
                id: label.into(),
            },
            label: format!("{label} (ASIO)"),
            what: what.into(),
            since: Instant::now(),
        });
        self.notice(
            "Guitar input lost".into(),
            "  ·  Line (NUX Audio): unplugged  ·  reconnecting…".into(),
        );
    }

    pub fn notice(&mut self, msg: String, tail: String) {
        self.notice = Some((msg, tail, Instant::now()));
    }

    /// A message said elsewhere at `at` (the app's toast), shown here on Home.
    pub fn notice_since(&mut self, msg: String, at: Instant) {
        self.notice = Some((msg, String::new(), at));
    }

    /// The notice (on Home) and the offer card, over the page.
    pub fn ui(&mut self, ui: &Ui, st: &mut AppState, area: Rect) {
        let th = t();
        let p = ui.painter().clone();
        let ctx = ui.ctx().clone();
        if self.offer.as_ref().is_some_and(|o| o.since.elapsed() > OFFER_FOR) {
            self.offer = None;
        }
        if self.notice.as_ref().is_some_and(|n| n.2.elapsed() > NOTICE_FOR) {
            self.notice = None;
        }
        // at the bottom right, floating over the page: the offer, and a notice above it
        let (pad, w) = (22.0, (400.0f32).min(area.width() - 40.0));
        let h = pad + 16.0 + 26.0 + 20.0 + 16.0 + 44.0 + pad;
        let card = Rect::from_min_size(pos2(area.max.x - 28.0 - w, area.max.y - 110.0 - h), vec2(w, h));
        if let Some((msg, tail, _)) = &self.notice {
            let tst = Style::new(16.0, 700, th.text);
            let tw = text::width(&ctx, msg, tst) + text::width(&ctx, tail, tst) + 44.0;
            let bottom = if self.offer.is_some() {
                card.min.y - 14.0
            } else {
                card.max.y
            };
            let th_ = text::line_height(&ctx, tst) + 20.0;
            widgets::toast(
                &p,
                msg,
                pos2(area.max.x - 28.0 - tw / 2.0, bottom - th_ / 2.0),
                tail,
            );
            ctx.request_repaint_after(Duration::from_millis(250));
        }
        let Some(o) = &self.offer else { return };
        ctx.request_repaint_after(Duration::from_millis(500));
        paint::floating(&p, card, Some(22.0));
        let x = card.min.x + pad;
        text::caps(
            &p,
            "GUITAR PLUGGED IN",
            pos2(x, card.min.y + pad),
            11.0,
            th.caption,
            Align2::LEFT_TOP,
            None,
        );
        text::draw(
            &p,
            &format!("Use {}?", o.label),
            pos2(x, card.min.y + pad + 16.0),
            Align2::LEFT_TOP,
            Style::new(17.0, 800, th.text),
            Some(card.width() - 2.0 * pad),
        );
        text::draw(
            &p,
            &o.what,
            pos2(x, card.min.y + pad + 16.0 + 26.0),
            Align2::LEFT_TOP,
            Style::new(12.0, 500, th.caption),
            Some(card.width() - 2.0 * pad),
        );
        let by = card.max.y - pad - 44.0;
        let st_ = ButtonStyle {
            size: 14.0,
            r: Some(14.0),
            ..Default::default()
        };
        let uw = widgets::button_width(&ctx, "Use it", "", 14.0, false);
        let nw = widgets::button_width(&ctx, "Not now", "", 14.0, false);
        let use_r = Rect::from_min_size(pos2(card.max.x - pad - uw, by), vec2(uw, 44.0));
        let not_r = Rect::from_min_size(pos2(use_r.min.x - 12.0 - nw, by), vec2(nw, 44.0));
        let not_now = widgets::button(ui, not_r, "devoffer-no", "Not now", "", st_).clicked();
        let take = widgets::button(
            ui,
            use_r,
            "devoffer-yes",
            "Use it",
            "",
            ButtonStyle { primary: true, ..st_ },
        )
        .clicked();
        if not_now {
            if let Some(o) = self.offer.take() {
                self.dismissed.push(o.id);
            }
        } else if take
            && let Some(o) = self.offer.take()
            && let Some(audio) = &st.audio
        {
            let mut prefs = audio.prefs();
            prefs.input = Choice::Device(o.pref);
            prefs.channel = None;
            audio.set_prefs(prefs.clone());
            st.config
                .set("audio", serde_json::to_value(&prefs).unwrap_or_default());
            st.save_config();
            self.notice(format!("Guitar input: {}", o.label), String::new());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn errors_are_cut_for_a_toast() {
        assert_eq!(short("  gone "), "gone");
        let long = "x".repeat(100);
        let s = short(&long);
        assert!(s.ends_with('…') && s.chars().count() == 69);
    }
}
