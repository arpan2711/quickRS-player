//! The highway's camera, planned over the whole part (owner request, D32; prototype in
//! `mg300-mods/highway/camera.js`).
//!
//! The old Python highway (rshighway.py, removed 2026-10-13) framed the anchors of the next second and chased them,
//! so it slid along the neck 11-29 times a minute and players got headaches. The chart is known ahead, so here the
//! path is planned once, like an L1-optimal video stabiliser: the part is cut into *holds* (one framing each) by
//! dynamic programming. A hold is feasible while every fretted note in it fits `max_frets`; each hold costs a move,
//! plus extra for being wide (zoomed out), for being short, and for starting without a gap before it. Inside a hold
//! the camera doesn't move. Between holds it makes one eased glide in the gap before the next hold's first note,
//! settled `lead` seconds before it. On the owner's songs: 0-4 moves a minute, still 15-300 s at a time, 99.5%+ of
//! notes in view (the old camera: 89-98%).
//!
//! Positions are in neck units ([`neck_x`]): fret 1 is 1 wide and frets narrow up the neck (half real spacing).

/// Where fret wire `f` sits (fractional frets fine): half equal spacing, half a real guitar's.
pub fn neck_x(f: f64) -> f64 {
    let real = 1.0 / (1.0 - 2f64.powf(-1.0 / 12.0));
    0.5 * f + 0.5 * real * (1.0 - 2f64.powf(-f / 12.0))
}

/// The fret wire at neck position `x` (inverse of [`neck_x`]).
pub fn fret_at(x: f64) -> f64 {
    let (mut a, mut b) = (-2.0, 30.0);
    for _ in 0..40 {
        let m = (a + b) / 2.0;
        if neck_x(m) < x {
            a = m;
        } else {
            b = m;
        }
    }
    (a + b) / 2.0
}

#[derive(Clone, Debug)]
pub struct CamOpts {
    /// Narrowest view (frets).
    pub min_frets: f64,
    /// Spans up to this cost nothing extra.
    pub comfort_frets: f64,
    /// A hold never needs more than this (hard; one chord wider than it gets a hold of its own).
    pub max_frets: f64,
    /// Margin each side of the outermost notes (frets).
    pub pad: f64,
    pub move_cost: f64,
    /// Per fret² above `comfort_frets`.
    pub zoom_cost: f64,
    /// Holds shorter than this (seconds) are penalised, up to `short_cost` for one of ~0 s.
    pub min_hold: f64,
    pub short_cost: f64,
    /// A move with less silence than `good_gap` before the hold's first note is penalised, up to `gap_cost`.
    pub gap_cost: f64,
    pub good_gap: f64,
    /// Seconds the camera is settled before a hold's first note.
    pub lead: f64,
    pub glide_min: f64,
    pub glide_max: f64,
    /// Holds whose frets fit this many together may glide while the earlier one is still playing.
    pub overlap_frets: f64,
    /// Frame the chart's hand window (anchors) too, when it still fits.
    pub use_anchors: bool,
}

impl Default for CamOpts {
    fn default() -> CamOpts {
        CamOpts {
            min_frets: 6.0,
            comfort_frets: 8.0,
            max_frets: 12.0,
            pad: 0.6,
            move_cost: 1.0,
            zoom_cost: 0.08,
            min_hold: 6.0,
            short_cost: 1.6,
            gap_cost: 0.8,
            good_gap: 0.8,
            lead: 0.4,
            glide_min: 0.6,
            glide_max: 3.0,
            overlap_frets: 13.0,
            use_anchors: true,
        }
    }
}

/// A moment the chart's fretting hand plays: its time, last sounding end, fretted range and the anchor's range.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CamEvent {
    pub t: f64,
    pub end: f64,
    pub lo: i32,
    pub hi: i32,
    /// The anchor (hand window) in force, `ahi < 0` when there is none.
    pub alo: i32,
    pub ahi: i32,
}

/// One framing held from `t0` to `t1` (first note to the last note's end), showing frets `lo..=hi`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hold {
    pub t0: f64,
    pub t1: f64,
    pub lo: i32,
    pub hi: i32,
}

/// The view's left and right edges at the now-line, in neck units.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct View {
    pub l: f64,
    pub r: f64,
}

#[derive(Clone, Copy, Debug)]
struct Key {
    t: f64,
    v: View,
}

/// A planned camera path: fixed views joined by eased glides.
#[derive(Clone, Debug)]
pub struct CamPlan {
    pub holds: Vec<Hold>,
    keys: Vec<Key>,
}

/// Note events from (time, fret, end) triples (chord notes share a time; open strings don't constrain the camera)
/// and anchors (start, fret, width), all sorted by time.
pub fn events(notes: &[(f64, i32, f64)], anchors: &[(f64, i32, i32)]) -> Vec<CamEvent> {
    let mut out: Vec<CamEvent> = Vec::new();
    for &(t, fret, end) in notes {
        let same = out.last().is_some_and(|e| (e.t - t).abs() <= 0.002);
        if !same {
            out.push(CamEvent {
                t,
                end: t,
                lo: 99,
                hi: -1,
                alo: 99,
                ahi: -1,
            });
        }
        let e = out.last_mut().expect("pushed");
        e.end = e.end.max(end);
        if fret > 0 {
            e.lo = e.lo.min(fret);
            e.hi = e.hi.max(fret);
        }
    }
    let mut ai = 0;
    for e in &mut out {
        while ai + 1 < anchors.len() && anchors[ai + 1].0 <= e.t + 0.001 {
            ai += 1;
        }
        if let Some(&(start, fret, width)) = anchors.get(ai)
            && start <= e.t + 0.001
        {
            e.alo = fret;
            e.ahi = fret + width.max(1) - 1;
        }
    }
    out.retain(|e| e.hi >= 0);
    out
}

impl CamPlan {
    /// Plan the camera for a part of `length` seconds.
    pub fn new(ev: &[CamEvent], length: f64, o: &CamOpts) -> CamPlan {
        let n = ev.len();
        if n == 0 {
            let v = frame(0, 4, o);
            return CamPlan {
                holds: Vec::new(),
                keys: vec![Key { t: f64::MIN, v }],
            };
        }
        let mut best = vec![f64::INFINITY; n + 1];
        let mut from = vec![0usize; n + 1];
        best[0] = 0.0;
        for j in 1..=n {
            let (mut lo, mut hi) = (99, -1);
            // the hold covers events i-1 ..= j-1 and lasts until the next hold starts (or the part ends)
            let t_end = if j < n { ev[j].t } else { length.max(ev[n - 1].end) };
            for i in (1..=j).rev() {
                let e = &ev[i - 1];
                lo = lo.min(e.lo);
                hi = hi.max(e.hi);
                let span = (hi - lo + 1) as f64 + 2.0 * o.pad;
                if span > o.max_frets && i < j {
                    break;
                }
                let dur = t_end - e.t;
                let wide = (span - o.comfort_frets).max(0.0);
                let gap = if i >= 2 { e.t - ev[i - 2].end } else { 9.0 };
                let c = best[i - 1]
                    + o.move_cost
                    + o.gap_cost * (1.0 - gap / o.good_gap).max(0.0)
                    + o.zoom_cost * wide * wide
                    + o.short_cost * (1.0 - dur / o.min_hold).max(0.0);
                if c < best[j] {
                    best[j] = c;
                    from[j] = i - 1;
                }
            }
        }
        let mut spans = Vec::new();
        let mut j = n;
        while j > 0 {
            spans.push((from[j], j - 1));
            j = from[j];
        }
        spans.reverse();
        let mut holds = Vec::with_capacity(spans.len());
        let mut views = Vec::with_capacity(spans.len());
        for &(a, b) in &spans {
            let (lo, hi) = hold_range(&ev[a..=b], o);
            holds.push(Hold {
                t0: ev[a].t,
                t1: ev[a..=b].iter().fold(ev[b].end, |m, e| m.max(e.end)),
                lo,
                hi,
            });
            views.push(frame(lo, hi, o));
        }
        let mut keys = vec![Key {
            t: f64::MIN,
            v: views[0],
        }];
        for k in 1..spans.len() {
            let last = &ev[spans[k - 1].1];
            let first = &ev[spans[k].0];
            let end = first.t - o.lead;
            // RS's camera drifts: a long eased move (longer the farther it goes). When both holds fit the
            // view together it may start during the earlier notes; otherwise it waits for the gap after them.
            let (a, b) = (holds[k - 1], holds[k]);
            let dist = (fret_at((views[k].l + views[k].r) / 2.0)
                - fret_at((views[k - 1].l + views[k - 1].r) / 2.0))
            .abs();
            let dur = (0.9 + 0.45 * dist).clamp(o.glide_min, o.glide_max);
            let both = (a.hi.max(b.hi) - a.lo.min(b.lo) + 1) as f64 + 2.0 * o.pad;
            let earliest = if both <= o.overlap_frets {
                a.t0
            } else {
                last.end.max(last.t + 0.05)
            };
            let mut start = earliest.max(end - dur);
            let mut stop = end;
            if stop - start < o.glide_min {
                // no room in the gap: the shortest glide, ending as near the settle point as the gap allows
                stop = (first.t - 0.05).min(end.max(last.t + o.glide_min));
                start = stop - o.glide_min;
            }
            // keys stay in time order even when holds come thick and fast
            let prev_t = keys.last().map_or(f64::MIN, |k| k.t);
            start = start.max(prev_t);
            stop = stop.max(start + 1e-3);
            keys.push(Key {
                t: start,
                v: views[k - 1],
            });
            keys.push(Key { t: stop, v: views[k] });
        }
        CamPlan { holds, keys }
    }

    /// The view at chart time `t`.
    pub fn at(&self, t: f64) -> View {
        let k = &self.keys;
        let i = k.partition_point(|x| x.t <= t);
        if i == 0 {
            return k[0].v;
        }
        if i >= k.len() {
            return k[k.len() - 1].v;
        }
        let (a, b) = (k[i - 1], k[i]);
        if a.v == b.v {
            return a.v;
        }
        let u = ((t - a.t) / (b.t - a.t)).clamp(0.0, 1.0);
        let u = u * u * u * (u * (u * 6.0 - 15.0) + 10.0); // smootherstep: eases in and out
        View {
            l: a.v.l + (b.v.l - a.v.l) * u,
            r: a.v.r + (b.v.r - a.v.r) * u,
        }
    }

    /// How many times the camera moves.
    pub fn moves(&self) -> usize {
        self.holds.len().saturating_sub(1)
    }
}

/// A hold's frets: its notes, widened to the hand windows in it when that still fits.
fn hold_range(ev: &[CamEvent], o: &CamOpts) -> (i32, i32) {
    let lo = ev.iter().map(|e| e.lo).min().unwrap_or(1);
    let hi = ev.iter().map(|e| e.hi).max().unwrap_or(4);
    if o.use_anchors {
        let alo = ev.iter().filter(|e| e.ahi >= 0).map(|e| e.alo).min();
        let ahi = ev.iter().filter(|e| e.ahi >= 0).map(|e| e.ahi).max();
        if let (Some(alo), Some(ahi)) = (alo, ahi) {
            let (ulo, uhi) = (lo.min(alo), hi.max(ahi));
            if (uhi - ulo + 1) as f64 + 2.0 * o.pad <= o.max_frets {
                return (ulo, uhi);
            }
        }
    }
    (lo, hi)
}

/// The view of frets `lo..=hi`: padded, at least `min_frets` wide, kept on the neck.
fn frame(lo: i32, hi: i32, o: &CamOpts) -> View {
    let mut fl = (lo - 1) as f64 - o.pad;
    let mut fr = hi as f64 + o.pad;
    let w = fr - fl;
    if w < o.min_frets {
        fl -= (o.min_frets - w) / 2.0;
        fr += (o.min_frets - w) / 2.0;
    }
    if fl < -0.6 {
        fr += -0.6 - fl;
        fl = -0.6;
    }
    if fr > 24.6 {
        fl -= fr - 24.6;
        fr = 24.6;
    }
    View {
        l: neck_x(fl),
        r: neck_x(fr),
    }
}

/// RS's own camera (fitted to the owner's reference video, Take Me Out, to 4.8 px RMS over every frame; scratch
/// camera/ report 2026-10-16). It frames the union of the hand's windows from the start of the current phrase to
/// 11.15 s ahead, zooms to fit that union (a 4-fret window ~9-12 frets across depending on where it is on the neck;
/// the nut never closer than ~12% of the screen from the left), and follows with a
/// critically damped spring: moves start early, take 6-7 s whatever the distance, blend into each other, and quick
/// hops inside the union never move it.
#[derive(Clone, Debug)]
pub struct RsCamera {
    t0: f64,
    /// (fret wire at the screen's middle, frets visible across the screen) at each step.
    path: Vec<(f32, f32)>,
}

const RS_STEP: f64 = 1.0 / 60.0;
// refit on nine reference videos (2026-10-16, scratch camera3/): pan better or equal on every song
const RS_DELAY: f64 = 0.0;
const RS_LOOKAHEAD: f64 = 11.15;
const RS_W_PAN: f64 = 0.56;
const RS_W_ZOOM: f64 = 0.526;

impl RsCamera {
    /// The target framing at time t: (wire at the screen's middle, frets across).
    fn target(anchors: &[(f64, f64, i32, i32)], phrases: &[f64], t: f64) -> Option<(f64, f64)> {
        let tau = t - RS_DELAY;
        let from = phrases[..phrases.partition_point(|&p| p <= tau)]
            .last()
            .copied()
            .unwrap_or(tau);
        let to = tau + RS_LOOKAHEAD;
        let (mut lo, mut hi) = (i32::MAX, i32::MIN);
        for &(start, end, fret, width) in anchors {
            if start > to {
                break;
            }
            if end > from {
                lo = lo.min(fret);
                hi = hi.max(fret + width.max(1) - 1);
            }
        }
        if hi < lo {
            // before the first window or past the last: the nearest one
            let a = anchors.iter().find(|a| a.0 > to).or(anchors.last())?;
            lo = a.2;
            hi = a.2 + a.3.max(1) - 1;
        }
        let w = (hi - lo + 1) as f64;
        // a 4-fret window shows ~9.2 frets low on the neck, more further up (frets narrow on the real guitar)
        let v = (10.785 + 0.255 * (lo - 7) as f64).max(2.043 + 1.501 * w);
        // the nut no closer than ~12% of the screen from the left edge
        let c = ((lo - 1 + hi) as f64 / 2.0 - 0.692 + 0.030 * v).max(v / 2.0 - (0.511 + 0.119 * v));
        Some((c, v))
    }

    /// Simulate the camera over a part: anchors (start, end, fret, width) sorted, phrase starts sorted (section starts
    /// will do). Each anchor is taken to end where the next starts (charts stitched from difficulty levels can have
    /// ends running past their successors).
    pub fn new(anchors: &[(f64, f64, i32, i32)], phrases: &[f64], length: f64) -> RsCamera {
        let mut fixed: Vec<(f64, f64, i32, i32)> = anchors.to_vec();
        for i in 0..fixed.len() {
            let next = fixed.get(i + 1).map_or(f64::INFINITY, |a| a.0);
            fixed[i].1 = next.max(fixed[i].0);
        }
        let anchors = &fixed[..];
        let t0 = anchors.first().map_or(0.0, |a| a.0.min(0.0)) - 2.0;
        let n = (((length + 2.0) - t0) / RS_STEP).ceil().max(1.0) as usize;
        let Some(first) = Self::target(anchors, phrases, t0) else {
            return RsCamera {
                t0,
                path: vec![(4.0, 12.0)],
            };
        };
        let (mut c, mut v) = first;
        let (mut dc, mut dv) = (0.0, 0.0);
        let mut goal = first;
        let mut path = Vec::with_capacity(n);
        for k in 0..n {
            let t = t0 + k as f64 * RS_STEP;
            if let Some(g) = Self::target(anchors, phrases, t) {
                goal = g;
            }
            // critically damped springs, semi-implicit Euler
            dc += (RS_W_PAN * RS_W_PAN * (goal.0 - c) - 2.0 * RS_W_PAN * dc) * RS_STEP;
            c += dc * RS_STEP;
            dv += (RS_W_ZOOM * RS_W_ZOOM * (goal.1 - v) - 2.0 * RS_W_ZOOM * dv) * RS_STEP;
            v += dv * RS_STEP;
            path.push((c as f32, v as f32));
        }
        RsCamera { t0, path }
    }

    /// (fret wire at the screen's middle, frets visible across) at time t.
    pub fn at(&self, t: f64) -> (f32, f32) {
        let x = ((t - self.t0) / RS_STEP).max(0.0);
        let i = (x.floor() as usize).min(self.path.len() - 1);
        let j = (i + 1).min(self.path.len() - 1);
        let u = (x - x.floor()).min(1.0) as f32;
        let (a, b) = (self.path[i], self.path[j]);
        (a.0 + (b.0 - a.0) * u, a.1 + (b.1 - a.1) * u)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn notes(spec: &[(f64, i32)]) -> Vec<(f64, i32, f64)> {
        spec.iter().map(|&(t, f)| (t, f, t + 0.1)).collect()
    }

    fn inside(v: View, fret: i32) -> bool {
        neck_x((fret - 1) as f64) >= v.l - 1e-9 && neck_x(fret as f64) <= v.r + 1e-9
    }

    #[test]
    fn neck_geometry() {
        assert!((neck_x(1.0) - 1.0).abs() < 1e-9);
        assert!(neck_x(13.0) - neck_x(12.0) < neck_x(1.0) - neck_x(0.0));
        assert!((fret_at(neck_x(7.3)) - 7.3).abs() < 1e-6);
    }

    #[test]
    fn small_moves_inside_a_position_dont_move_the_camera() {
        // frets 3-7 wandering for a minute, a note every 0.25 s: one hold, never a move
        let spec: Vec<(f64, i32)> = (0..240).map(|i| (i as f64 * 0.25, 3 + (i * 7 % 5))).collect();
        let ev = events(&notes(&spec), &[]);
        let plan = CamPlan::new(&ev, 60.0, &CamOpts::default());
        assert_eq!(plan.moves(), 0);
        for &(t, f) in &spec {
            assert!(inside(plan.at(t), f), "fret {f} at {t}");
        }
    }

    #[test]
    fn a_real_position_change_moves_once_in_the_gap() {
        // 20 s at frets 2-5, a 1.5 s rest, 20 s at frets 14-17
        let mut spec: Vec<(f64, i32)> = (0..80).map(|i| (i as f64 * 0.25, 2 + i % 4)).collect();
        spec.extend((0..80).map(|i| (21.5 + i as f64 * 0.25, 14 + i % 4)));
        let ev = events(&notes(&spec), &[]);
        let o = CamOpts::default();
        let plan = CamPlan::new(&ev, 45.0, &o);
        assert_eq!(plan.moves(), 1);
        // still through the first stretch, moving only in the rest, settled before the next note
        assert_eq!(plan.at(0.0), plan.at(19.8));
        assert_ne!(plan.at(20.5), plan.at(19.8));
        assert_eq!(plan.at(21.5 - o.lead), plan.at(40.0));
        for &(t, f) in &spec {
            assert!(inside(plan.at(t), f), "fret {f} at {t}");
        }
    }

    #[test]
    fn one_stray_note_widens_rather_than_chases() {
        // frets 5-8 with one note at 10 in the middle: one wider hold, not three
        let mut spec: Vec<(f64, i32)> = (0..60).map(|i| (i as f64 * 0.25, 5 + i % 4)).collect();
        spec[30].1 = 10;
        let plan = CamPlan::new(&events(&notes(&spec), &[]), 16.0, &CamOpts::default());
        assert_eq!(plan.moves(), 0);
        assert!(inside(plan.at(7.5), 10));
    }

    #[test]
    fn open_strings_and_anchors() {
        // open strings alone don't need a view; the anchor widens the frame when it fits
        let ev = events(&[(0.0, 0, 0.1), (1.0, 5, 1.1), (1.0, 7, 1.1)], &[(0.0, 5, 4)]);
        assert_eq!(ev.len(), 1);
        assert_eq!((ev[0].lo, ev[0].hi, ev[0].alo, ev[0].ahi), (5, 7, 5, 8));
        let plan = CamPlan::new(&ev, 10.0, &CamOpts::default());
        assert_eq!((plan.holds[0].lo, plan.holds[0].hi), (5, 8));
        assert!(inside(plan.at(1.0), 8));
    }

    #[test]
    fn empty_part_has_a_view() {
        let plan = CamPlan::new(&[], 10.0, &CamOpts::default());
        let v = plan.at(3.0);
        assert!(v.r > v.l);
    }

    #[test]
    fn rs_camera_settles_on_the_window_and_ignores_hops_inside_it() {
        // windows at 7 for a minute: it settles on 7..10 at ~10.8 frets, the window's middle (wire 8) left of centre
        let a = vec![(0.0, 60.0, 7, 4)];
        let cam = RsCamera::new(&a, &[], 60.0);
        let (c, v) = cam.at(50.0);
        assert!((v - 10.785).abs() < 0.05, "{v}");
        assert!((c as f64 - (8.0 - 0.692 + 0.030 * 10.785)).abs() < 0.05, "{c}");
        // hopping 5 <-> 7 every 1.5 s inside one phrase: the target never changes, so the camera never moves
        let hops: Vec<_> = (0..40)
            .map(|i| {
                (
                    i as f64 * 1.5,
                    i as f64 * 1.5 + 1.5,
                    if i % 2 == 0 { 5 } else { 7 },
                    4,
                )
            })
            .collect();
        let cam = RsCamera::new(&hops, &[0.0], 60.0);
        let (c1, _) = cam.at(20.0);
        let (c2, _) = cam.at(40.0);
        assert!((c1 - c2).abs() < 0.01, "{c1} {c2}");
    }

    #[test]
    fn rs_camera_moves_slowly_and_starts_early() {
        // 3..6 until 30 s, then 12..15: it starts moving about 9 s ahead and takes seconds, without overshoot
        let a = vec![(0.0, 30.0, 3, 4), (30.0, 60.0, 12, 4)];
        let cam = RsCamera::new(&a, &[0.0, 30.0], 60.0);
        let (c0, _) = cam.at(15.0);
        let (c_mid, _) = cam.at(25.0);
        let (c_end, _) = cam.at(55.0);
        assert!(c_mid > c0 + 0.5, "moving before the change: {c0} {c_mid}");
        let peak = (0..3600)
            .map(|k| cam.at(k as f64 / 60.0).0)
            .fold(f32::MIN, f32::max);
        assert!(peak <= c_end + 0.05, "no overshoot: {peak} {c_end}");
    }
}
