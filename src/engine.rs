//! Show rendering: (project, song, time) -> per-fixture state -> DMX universe.

use crate::model::*;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FxState {
    /// Color 0..1 (before dimmer).
    pub rgb: [f32; 3],
    /// Intensity 0..1.
    pub dim: f32,
    /// Degrees, 0 = home.
    pub pan: f32,
    /// Degrees from vertical, + = towards the audience.
    pub tilt: f32,
    /// 0 = no strobe, 1 = fastest.
    pub strobe: f32,
}

impl FxState {
    fn lerp(a: &FxState, b: &FxState, k: f32) -> FxState {
        let l = |x: f32, y: f32| x + (y - x) * k;
        FxState {
            rgb: [l(a.rgb[0], b.rgb[0]), l(a.rgb[1], b.rgb[1]), l(a.rgb[2], b.rgb[2])],
            dim: l(a.dim, b.dim),
            pan: l(a.pan, b.pan),
            tilt: l(a.tilt, b.tilt),
            strobe: if k < 0.5 { a.strobe } else { b.strobe },
        }
    }
}

pub fn c8(c: [u8; 3]) -> [f32; 3] {
    [c[0] as f32 / 255.0, c[1] as f32 / 255.0, c[2] as f32 / 255.0]
}

fn hash(a: u64) -> f32 {
    let mut x = a.wrapping_mul(0x9E3779B97F4A7C15);
    x ^= x >> 31;
    x = x.wrapping_mul(0xBF58476D1CE4E5B9);
    x ^= x >> 29;
    (x % 10000) as f32 / 10000.0
}

fn last_before(v: &[f64], t: f64) -> Option<f64> {
    let i = v.partition_point(|&x| x <= t);
    if i == 0 {
        None
    } else {
        Some(v[i - 1])
    }
}

/// Order of each fixture inside its group (left to right) and the group size.
pub fn group_positions(p: &Project) -> Vec<(usize, usize)> {
    let mut out = vec![(0usize, 1usize); p.fixtures.len()];
    for g in Group::ALL {
        let mut idx: Vec<usize> = (0..p.fixtures.len()).filter(|&i| p.fixtures[i].group == g).collect();
        idx.sort_by(|a, b| p.fixtures[*a].x.total_cmp(&p.fixtures[*b].x));
        let n = idx.len();
        for (k, i) in idx.into_iter().enumerate() {
            out[i] = (k, n);
        }
    }
    out
}

fn nearest_wheel(def: &FixtureDef, rgb: [f32; 3]) -> Option<&ColorSlot> {
    let m = rgb[0].max(rgb[1]).max(rgb[2]).max(1e-3);
    let n = [rgb[0] / m, rgb[1] / m, rgb[2] / m];
    def.color_wheel.iter().min_by(|a, b| {
        let d = |s: &ColorSlot| {
            let c = c8(s.color);
            let cm = c[0].max(c[1]).max(c[2]).max(1e-3);
            (0..3).map(|i| (c[i] / cm - n[i]).powi(2)).sum::<f32>()
        };
        d(a).total_cmp(&d(b))
    })
}

#[allow(clippy::too_many_arguments)]
fn look_state(
    look: &GroupLook,
    def: Option<&FixtureDef>,
    inst: &FixtureInstance,
    k: usize,
    n: usize,
    beat: f64,
    t: f64,
    ana: Option<&Analysis>,
) -> FxState {
    let nf = n.max(1) as f64;
    let rate = look.rate.max(0.125) as f64;
    let ncol = look.colors.len().max(1);
    let col_idx = match look.effect {
        Effect::ColorChase => (k + (beat / rate).floor().max(0.0) as usize) % ncol,
        _ => k % ncol,
    };
    let rgb = look.colors.get(col_idx).map(|c| c8(*c)).unwrap_or([1.0, 1.0, 1.0]);
    let spb = ana.map(|a| a.spb()).unwrap_or(0.5);
    let pulse_env = |dt: f64| (0.2 + 0.8 * (-dt / (0.35 * spb).max(0.05)).exp()) as f32;
    let m: f32 = match look.effect {
        Effect::Static | Effect::ColorChase => 1.0,
        Effect::Off => 0.0,
        Effect::Pulse => {
            let ph = (beat / rate).rem_euclid(1.0);
            pulse_env(ph * rate * spb)
        }
        Effect::Kick | Effect::Snare => {
            let list = ana.map(|a| if look.effect == Effect::Kick { &a.kicks } else { &a.snares });
            match list.and_then(|l| last_before(l, t)) {
                Some(h) if !list.unwrap().is_empty() => pulse_env(t - h),
                _ => {
                    let ph = (beat / rate).rem_euclid(1.0);
                    pulse_env(ph * rate * spb)
                }
            }
        }
        Effect::Chase => {
            let pos = (beat / rate).rem_euclid(nf);
            let d = (k as f64 - pos).rem_euclid(nf);
            let d = d.min(nf - d);
            (1.0 - d / 1.2).clamp(0.08, 1.0) as f32
        }
        Effect::Alternate => {
            let c = (beat / rate).floor().rem_euclid(2.0) as usize;
            if k % 2 == c {
                1.0
            } else {
                0.1
            }
        }
        Effect::Breath => {
            let ph = beat / rate + look.spread as f64 * k as f64 / nf;
            (0.3 + 0.7 * (0.5 - 0.5 * (ph * std::f64::consts::TAU).cos())) as f32
        }
        Effect::Sparkle => {
            let slot = (t * 10.0).floor() as u64;
            0.15 + 0.85 * hash(slot * 131 + k as u64 * 7 + inst.id as u64).powf(2.0)
        }
    };
    let mut st = FxState { rgb, dim: look.intensity * m, pan: inst.aim_pan, tilt: inst.aim_tilt, strobe: look.strobe };
    let moving = def.map(|d| d.kind.is_moving()).unwrap_or(false);
    if moving {
        let s = look.move_size;
        // mirrored: left and right heads share the phase of their symmetric partner
        let kp = if look.mirror { k.min(n.saturating_sub(1).saturating_sub(k)) } else { k };
        let ph = beat / look.move_rate.max(0.25) as f64 + look.spread as f64 * kp as f64 / nf;
        let w = (ph * std::f64::consts::TAU) as f32;
        let side = if nf > 1.0 { k as f32 / (nf as f32 - 1.0) * 2.0 - 1.0 } else { 0.0 };
        let k = kp;
        let (bp, bt) = (inst.aim_pan, look.tilt);
        let (p, tl) = match look.movement {
            Movement::Still => (bp, bt),
            Movement::Circle => (bp + 40.0 * s * w.sin(), bt + 22.0 * s * w.cos()),
            Movement::Figure8 => (bp + 45.0 * s * w.sin(), bt + 18.0 * s * (2.0 * w).sin()),
            Movement::PanSweep => (bp + 65.0 * s * w.sin(), bt),
            Movement::TiltSweep => (bp + side * 15.0, bt + 35.0 * s * w.sin()),
            Movement::Ballyhoo => (
                bp + 75.0 * s * (w + k as f32 * 1.3).sin(),
                bt + 35.0 * s * (1.7 * w + k as f32 * 2.1).sin(),
            ),
            Movement::Crowd => (bp + side * 25.0 + 10.0 * s * w.sin(), 72.0 + 8.0 * s * w.cos()),
            Movement::Fan => (bp + side * 50.0 * s.max(0.2), bt),
        };
        let left = inst.x < -0.05;
        st.pan = if look.mirror && left && !matches!(look.movement, Movement::Fan | Movement::Crowd) { bp - (p - bp) } else { p };
        st.tilt = tl;
    }
    if let Some(d) = def {
        if d.has(ChannelKind::ColorWheel) && !d.has_rgb() {
            if let Some(slot) = nearest_wheel(d, st.rgb) {
                st.rgb = c8(slot.color);
            }
        } else if !d.has_rgb() && !d.has(ChannelKind::ColorWheel) {
            // white-only fixtures (blinders, strobes, dimmers)
            st.rgb = if d.kind == FixtureKind::Blinder { [1.0, 0.82, 0.55] } else { [1.0, 1.0, 1.0] };
        }
    }
    st
}

pub struct RenderInput<'a> {
    pub project: &'a Project,
    pub catalog: &'a [FixtureDef],
    pub song: Option<&'a Song>,
    pub t: f64,
    pub blackout: bool,
    pub identify: Option<Vec<u32>>,
}

/// Mix a flash colour over the current light output (additive look, no colour "sticking").
fn overlay(s: &mut FxState, col: [f32; 3], env: f32) {
    let env = env.clamp(0.0, 1.0);
    let mut l = [0f32; 3];
    for i in 0..3 {
        l[i] = s.rgb[i] * s.dim * (1.0 - env) + col[i] * env;
    }
    let m = l[0].max(l[1]).max(l[2]);
    if m > 1e-4 {
        s.dim = m.min(1.0);
        s.rgb = [l[0] / m, l[1] / m, l[2] / m];
    }
}

pub fn render(inp: &RenderInput) -> Vec<FxState> {
    let p = inp.project;
    let pos = group_positions(p);
    let mut out = vec![FxState::default(); p.fixtures.len()];
    for (i, f) in p.fixtures.iter().enumerate() {
        out[i].pan = f.aim_pan;
        out[i].tilt = f.aim_tilt;
        out[i].rgb = [1.0, 1.0, 1.0];
    }
    if let Some(ids) = &inp.identify {
        for (i, f) in p.fixtures.iter().enumerate() {
            if ids.contains(&f.id) {
                out[i].dim = 1.0;
                out[i].rgb = [1.0, 1.0, 1.0];
            }
        }
        return out;
    }
    let Some(song) = inp.song else { return out };
    if inp.blackout {
        return out;
    }
    let t = inp.t;
    let ana = song.analysis.as_ref();
    let beat = ana.map(|a| a.beat_at(t)).unwrap_or(t * 2.0);
    let ci = song.cues.partition_point(|c| c.time <= t);
    if ci > 0 {
        let cue = &song.cues[ci - 1];
        let compute = |cue: &Cue| -> Vec<FxState> {
            p.fixtures
                .iter()
                .enumerate()
                .map(|(i, f)| {
                    let look = &cue.looks[f.group.idx().min(cue.looks.len().saturating_sub(1))];
                    let def = p.def(inp.catalog, &f.def_id);
                    look_state(look, def, f, pos[i].0, pos[i].1, beat, t, ana)
                })
                .collect()
        };
        let cur = compute(cue);
        let fade_k = if cue.fade > 0.0 { ((t - cue.time) / cue.fade).clamp(0.0, 1.0) as f32 } else { 1.0 };
        out = if fade_k < 1.0 {
            let prev = if ci >= 2 { compute(&song.cues[ci - 2]) } else { out.clone() };
            prev.iter().zip(&cur).map(|(a, b)| FxState::lerp(a, b, fade_k)).collect()
        } else {
            cur
        };
    }
    // hits: blackouts first, then flashes
    let start = song.hits.partition_point(|h| h.time < t - 6.0);
    for h in &song.hits[start..] {
        if h.time > t {
            break;
        }
        if h.kind == HitKind::Blackout && t < h.time + h.duration {
            for s in &mut out {
                s.dim = 0.0;
                s.strobe = 0.0;
            }
        }
    }
    for h in &song.hits[start..] {
        if h.time > t {
            break;
        }
        let dt = t - h.time;
        // short tail: blinders fade like a hot lamp, flashes stop almost at once
        let tail = if h.kind == HitKind::Blinder { 0.2 } else { 0.06 };
        let env = if dt < h.duration { 1.0 - 0.2 * (dt / h.duration.max(0.01)) as f32 } else { (-(dt - h.duration) / tail).exp() as f32 * 0.8 };
        if env < 0.02 {
            continue;
        }
        match h.kind {
            HitKind::Blinder => {
                let has_blinders = p.fixtures.iter().any(|f| f.group == Group::Blinders);
                for (i, f) in p.fixtures.iter().enumerate() {
                    if f.group == Group::Blinders || (!has_blinders && f.group == Group::BackWash) {
                        overlay(&mut out[i], [1.0, 0.82, 0.55], env);
                    }
                }
            }
            HitKind::Flash => {
                for s in out.iter_mut() {
                    overlay(s, [1.0, 1.0, 1.0], env);
                }
            }
            HitKind::Strobe => {
                if dt < h.duration {
                    let has = p.fixtures.iter().any(|f| f.group == Group::Strobes);
                    for (i, f) in p.fixtures.iter().enumerate() {
                        if f.group == Group::Strobes || (!has && matches!(f.group, Group::BackWash | Group::FrontWash)) {
                            out[i].dim = 1.0;
                            out[i].strobe = 1.0;
                            out[i].rgb = [1.0, 1.0, 1.0];
                        }
                    }
                }
            }
            HitKind::Blackout => {}
        }
    }
    let master = p.settings.master.clamp(0.0, 1.0);
    for s in &mut out {
        s.dim = (s.dim * master).clamp(0.0, 1.0);
    }
    out
}

/// Write a fixture's state into the DMX universe.
pub fn write_dmx(def: &FixtureDef, inst: &FixtureInstance, st: &FxState, uni: &mut [u8; 512]) {
    let has_dimmer = def.has(ChannelKind::Dimmer);
    let mut rgb = st.rgb;
    let mut white = 0f32;
    if def.has(ChannelKind::White) && def.has_rgb() {
        let w = rgb[0].min(rgb[1]).min(rgb[2]);
        white = w;
        for c in &mut rgb {
            *c -= w;
        }
    }
    let scale = if has_dimmer { 1.0 } else { st.dim };
    let to8 = |x: f32| (x.clamp(0.0, 1.0) * 255.0).round() as u8;
    let pan16 = (((st.pan + def.pan_range * 0.5) / def.pan_range.max(1.0)).clamp(0.0, 1.0) * 65535.0) as u16;
    let tilt16 = (((st.tilt + def.tilt_range * 0.5) / def.tilt_range.max(1.0)).clamp(0.0, 1.0) * 65535.0) as u16;
    for (i, c) in def.channels.iter().enumerate() {
        let a = inst.address as usize + i;
        if a == 0 || a > 512 {
            continue;
        }
        let v = match c.kind {
            ChannelKind::Dimmer => to8(st.dim),
            ChannelKind::Red => to8(rgb[0] * scale),
            ChannelKind::Green => to8(rgb[1] * scale),
            ChannelKind::Blue => to8(rgb[2] * scale),
            ChannelKind::White => {
                if def.has_rgb() {
                    to8(white * scale)
                } else {
                    to8(st.dim)
                }
            }
            ChannelKind::Amber => {
                let a = (st.rgb[0].min(st.rgb[1] * 1.6) - st.rgb[2]).max(0.0);
                to8(a * scale * 0.8)
            }
            ChannelKind::Uv => 0,
            ChannelKind::Pan => (pan16 >> 8) as u8,
            ChannelKind::PanFine => (pan16 & 0xff) as u8,
            ChannelKind::Tilt => (tilt16 >> 8) as u8,
            ChannelKind::TiltFine => (tilt16 & 0xff) as u8,
            ChannelKind::Shutter => {
                if st.strobe > 0.0 {
                    let lo = def.strobe_min as f32;
                    let hi = def.strobe_max as f32;
                    (lo + (hi - lo) * st.strobe.clamp(0.0, 1.0)) as u8
                } else if st.dim <= 0.001 && !has_dimmer {
                    def.shutter_closed
                } else {
                    def.shutter_open
                }
            }
            ChannelKind::Strobe => {
                if st.strobe > 0.0 {
                    (10.0 + 245.0 * st.strobe.clamp(0.0, 1.0)) as u8
                } else {
                    0
                }
            }
            ChannelKind::ColorWheel => nearest_wheel(def, st.rgb).map(|s| s.value).unwrap_or(c.default),
            _ => c.default,
        };
        uni[a - 1] = v;
    }
}

pub fn universe(p: &Project, catalog: &[FixtureDef], states: &[FxState]) -> [u8; 512] {
    let mut u = [0u8; 512];
    for (f, s) in p.fixtures.iter().zip(states) {
        if let Some(d) = p.def(catalog, &f.def_id) {
            write_dmx(d, f, s, &mut u);
        }
    }
    u
}
