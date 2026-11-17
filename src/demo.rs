//! A synthetic multitrack demo song and a demo rig, so the app can be tried
//! without any audio files. The demo audio goes through the real analysis.

use crate::model::*;

pub const DEMO_BPM: f64 = 124.0;
const SR: u32 = 22050;
const BARS: usize = 82;

#[derive(Clone, Copy, PartialEq)]
enum Part {
    Intro,
    Verse,
    Pre,
    Chorus,
    Stop,
    Outro,
}

fn part(bar: usize) -> Part {
    match bar {
        0..=7 => Part::Intro,
        8..=23 => Part::Verse,
        24..=27 => Part::Pre,
        28..=43 => Part::Chorus,
        44..=45 => Part::Stop,
        46..=53 => Part::Verse,
        54..=57 => Part::Pre,
        58..=73 => Part::Chorus,
        _ => Part::Outro,
    }
}

struct Rng(u32);
impl Rng {
    fn next(&mut self) -> f32 {
        self.0 = self.0.wrapping_mul(1664525).wrapping_add(1013904223);
        ((self.0 >> 9) as f32 / (1u32 << 23) as f32) * 2.0 - 1.0
    }
}

fn spb() -> f64 {
    60.0 / DEMO_BPM
}
fn total_len() -> usize {
    ((BARS as f64 * 4.0 * spb() + 1.5) * SR as f64) as usize
}
fn at(t: f64) -> usize {
    (t * SR as f64) as usize
}

fn add<F: FnMut(f32) -> f32>(buf: &mut [f32], start: f64, dur: f64, mut f: F) {
    let a = at(start);
    let n = at(dur);
    for i in 0..n {
        if a + i >= buf.len() {
            break;
        }
        let t = i as f32 / SR as f32;
        buf[a + i] += f(t);
    }
}

const ROOTS: [f32; 4] = [55.0, 43.65, 65.41, 49.0]; // A F C G

fn kick(buf: &mut [f32], t0: f64, amp: f32) {
    let mut ph = 0f32;
    add(buf, t0, 0.35, |t| {
        let f = 45.0 + 90.0 * (-t / 0.04).exp();
        ph += f / SR as f32;
        (ph * std::f32::consts::TAU).sin() * (-t / 0.13).exp() * amp
    });
}
fn snare(buf: &mut [f32], t0: f64, amp: f32, rng: &mut Rng) {
    add(buf, t0, 0.25, |t| {
        (rng.next() * 0.6 * (-t / 0.07).exp() + (t * 190.0 * std::f32::consts::TAU).sin() * 0.4 * (-t / 0.04).exp()) * amp
    });
}
fn hat(buf: &mut [f32], t0: f64, amp: f32, rng: &mut Rng) {
    let mut prev = 0f32;
    add(buf, t0, 0.06, |t| {
        let n = rng.next();
        let h = n - prev;
        prev = n;
        h * (-t / 0.015).exp() * amp
    });
}
fn crash(buf: &mut [f32], t0: f64, amp: f32, rng: &mut Rng) {
    let mut prev = 0f32;
    add(buf, t0, 1.6, |t| {
        let n = rng.next();
        let h = n - prev;
        prev = n;
        h * (-t / 0.7).exp() * amp
    });
}

fn drums() -> Vec<f32> {
    let mut b = vec![0f32; total_len()];
    let mut rng = Rng(12345);
    let spb = spb();
    for bar in 0..BARS {
        let p = part(bar);
        let t_bar = bar as f64 * 4.0 * spb;
        let prev = if bar > 0 { part(bar - 1) } else { Part::Intro };
        if p != prev && matches!(p, Part::Chorus | Part::Verse) {
            crash(&mut b, t_bar, 0.5, &mut rng);
        }
        for step in 0..16 {
            let t = t_bar + step as f64 * spb / 4.0;
            let beat = step / 4;
            let on_beat = step % 4 == 0;
            match p {
                Part::Intro => {
                    if bar >= 4 && step % 2 == 0 {
                        hat(&mut b, t, 0.12, &mut rng);
                    }
                    if bar >= 6 && on_beat && beat % 2 == 0 {
                        kick(&mut b, t, 0.5);
                    }
                }
                Part::Verse => {
                    if on_beat && (beat == 0 || beat == 2) || step == 10 {
                        kick(&mut b, t, 0.75);
                    }
                    if on_beat && (beat == 1 || beat == 3) {
                        snare(&mut b, t, 0.55, &mut rng);
                    }
                    if step % 2 == 0 {
                        hat(&mut b, t, 0.15, &mut rng);
                    }
                }
                Part::Pre => {
                    if on_beat {
                        kick(&mut b, t, 0.8);
                    }
                    let last = bar % 4 == 3;
                    if (last && step >= 8) || (on_beat && (beat == 1 || beat == 3)) || (step % 2 == 0 && bar % 4 == 2) {
                        snare(&mut b, t, 0.45 + step as f32 * 0.02, &mut rng);
                    }
                }
                Part::Chorus => {
                    if on_beat {
                        kick(&mut b, t, 1.0);
                    }
                    if on_beat && (beat == 1 || beat == 3) {
                        snare(&mut b, t, 0.75, &mut rng);
                    }
                    hat(&mut b, t, 0.2, &mut rng);
                    if step == 0 && bar % 4 == 0 && bar != 28 && bar != 58 {
                        crash(&mut b, t, 0.35, &mut rng);
                    }
                }
                Part::Stop => {
                    if bar == 44 && step == 0 {
                        kick(&mut b, t, 1.0);
                        snare(&mut b, t, 0.8, &mut rng);
                        crash(&mut b, t, 0.6, &mut rng);
                    }
                    if bar == 45 && step >= 12 {
                        snare(&mut b, t, 0.7, &mut rng);
                    }
                }
                Part::Outro => {
                    let fade = 1.0 - (bar - 74) as f32 / 8.0;
                    if on_beat && beat == 0 {
                        kick(&mut b, t, 0.6 * fade);
                    }
                    if step % 4 == 2 {
                        hat(&mut b, t, 0.1 * fade, &mut rng);
                    }
                }
            }
        }
    }
    b
}

fn saw(ph: f32) -> f32 {
    2.0 * ph.fract() - 1.0
}

fn bass() -> Vec<f32> {
    let mut b = vec![0f32; total_len()];
    let spb = spb();
    for bar in 0..BARS {
        let p = part(bar);
        let root = ROOTS[(bar / 2) % 4];
        let t_bar = bar as f64 * 4.0 * spb;
        let (amp, steps): (f32, &[usize]) = match p {
            Part::Intro => (if bar >= 6 { 0.25 } else { 0.0 }, &[0, 4]),
            Part::Verse => (0.35, &[0, 2, 4, 6]),
            Part::Pre => (0.4, &[0, 1, 2, 3, 4, 5, 6, 7]),
            Part::Chorus => (0.5, &[0, 1, 2, 3, 4, 5, 6, 7]),
            Part::Stop => (if bar == 44 { 0.6 } else { 0.0 }, &[0]),
            Part::Outro => (0.2 * (1.0 - (bar - 74) as f32 / 8.0), &[0]),
        };
        if amp <= 0.0 {
            continue;
        }
        for &s in steps {
            let t = t_bar + s as f64 * spb / 2.0;
            let len = if p == Part::Stop { 0.3 } else { spb / 2.0 * 0.95 };
            let mut ph = 0f32;
            add(&mut b, t, len, |tt| {
                ph += root / SR as f32;
                let env = (tt / 0.005).min(1.0) * (-tt / 0.4).exp();
                (saw(ph) * 0.6 + (ph * std::f32::consts::TAU).sin() * 0.6) * env * amp
            });
        }
    }
    b
}

fn synth() -> Vec<f32> {
    let mut b = vec![0f32; total_len()];
    let spb = spb();
    for bar in 0..BARS {
        let p = part(bar);
        let root = ROOTS[(bar / 2) % 4] * 4.0;
        let t_bar = bar as f64 * 4.0 * spb;
        let amp = match p {
            Part::Chorus => 0.22,
            Part::Pre => 0.1,
            Part::Stop if bar == 44 => 0.3,
            _ => 0.0,
        };
        if amp <= 0.0 {
            continue;
        }
        let hits: Vec<usize> = if p == Part::Stop { vec![0] } else { (0..8).collect() };
        for s in hits {
            let t = t_bar + s as f64 * spb / 2.0;
            let len = if p == Part::Stop { 0.4 } else { spb / 2.0 };
            let (mut a, mut c, mut d) = (0f32, 0f32, 0f32);
            add(&mut b, t, len, |tt| {
                a += root / SR as f32;
                c += root * 1.26 / SR as f32;
                d += root * 1.5 / SR as f32;
                let env = (tt / 0.004).min(1.0) * (-tt / 0.18).exp();
                (saw(a) + saw(c) * 0.8 + saw(d) * 0.7) * env * amp
            });
        }
    }
    b
}

fn pad() -> Vec<f32> {
    let mut b = vec![0f32; total_len()];
    let spb = spb();
    for bar in 0..BARS {
        let p = part(bar);
        let amp = match p {
            Part::Intro => 0.18,
            Part::Verse => 0.08,
            Part::Outro => 0.18 * (1.0 - (bar - 74) as f32 / 9.0),
            _ => 0.0,
        };
        if amp <= 0.0 {
            continue;
        }
        let root = ROOTS[(bar / 2) % 4] * 4.0;
        let t_bar = bar as f64 * 4.0 * spb;
        let len = 4.0 * spb;
        add(&mut b, t_bar, len, |tt| {
            let env = (tt / 0.3).min(1.0) * ((len as f32 - tt) / 0.3).clamp(0.0, 1.0);
            let s = (tt * root * std::f32::consts::TAU).sin()
                + (tt * root * 1.2 * std::f32::consts::TAU).sin() * 0.7
                + (tt * root * 1.5 * std::f32::consts::TAU).sin() * 0.6;
            s * env * amp
        });
    }
    b
}

fn click() -> Vec<f32> {
    let mut b = vec![0f32; total_len()];
    let spb = spb();
    for beat in 0..BARS * 4 {
        let f = if beat % 4 == 0 { 1500.0 } else { 1000.0 };
        add(&mut b, beat as f64 * spb, 0.03, |t| (t * f * std::f32::consts::TAU).sin() * (-t / 0.008).exp() * 0.5);
    }
    b
}

pub fn synth_track(name: &str) -> (u32, Vec<f32>) {
    let v = match name {
        "batteria" => drums(),
        "basso" => bass(),
        "synth" => synth(),
        "pad" => pad(),
        "click" => click(),
        _ => vec![0.0; total_len()],
    };
    (SR, v)
}

pub fn demo_tracks() -> Vec<TrackInfo> {
    [
        ("batteria", "Batteria", TrackRole::Drums),
        ("basso", "Basso", TrackRole::Bass),
        ("synth", "Synth", TrackRole::Keys),
        ("pad", "Pad", TrackRole::Keys),
        ("click", "Click", TrackRole::Click),
    ]
    .iter()
    .map(|(id, name, role)| TrackInfo { path: format!("demo:{id}"), name: crate::i18n::t(name), role: *role, env: vec![], hidden: false })
    .collect()
}

fn inst(p: &mut Project, def: &str, name: &str, x: f32, y: f32, z: f32, hanging: bool, group: Group, aim_tilt: f32, addr: u16) {
    let id = p.next_id;
    p.next_id += 1;
    p.fixtures.push(FixtureInstance {
        id,
        def_id: def.to_string(),
        name: name.to_string(),
        address: addr,
        x,
        y,
        z,
        hanging,
        group,
        aim_pan: 0.0,
        aim_tilt,
    });
}

/// Demo rig: wash in front, PAR backlight, blinders, four moving heads, a strobe, LED bars.
pub fn demo_rig(p: &mut Project) {
    let st = p.stage.clone();
    let mut addr = 1u16;
    let mut next = |n: u16| {
        let a = addr;
        addr += n;
        a
    };
    for i in 0..4 {
        let x = -3.0 + i as f32 * 2.0;
        let a = next(7);
        inst(p, "par-rgb-7", &crate::i18n::t(&format!("Wash {}", i + 1)), x, 0.4, st.truss_front, true, Group::FrontWash, -35.0, a);
    }
    for i in 0..6 {
        let x = -4.0 + i as f32 * 1.6;
        let a = next(8);
        inst(p, "par-rgbw-8", &crate::i18n::t(&format!("Controluce {}", i + 1)), x, st.depth - 0.4, st.truss_back, true, Group::BackWash, 20.0, a);
    }
    for (i, x) in [-2.5f32, 2.5].iter().enumerate() {
        let a = next(4);
        inst(p, "blinder-4", &crate::i18n::t(&format!("Accecatore {}", i + 1)), *x, st.depth - 0.3, 0.4, false, Group::Blinders, 75.0, a);
    }
    for (i, x) in [-3.5f32, 3.5].iter().enumerate() {
        let a = next(11);
        inst(p, "head-spot-11", &format!("Spot {}", i + 1), *x, 1.0, 0.3, false, Group::HeadsFront, 0.0, a);
    }
    for (i, x) in [-1.5f32, 1.5].iter().enumerate() {
        let a = next(16);
        inst(p, "head-beam-16", &format!("Beam {}", i + 1), *x, st.depth - 0.6, st.truss_back, true, Group::HeadsBack, 0.0, a);
    }
    let a = next(2);
    inst(p, "strobe-2", crate::i18n::ts("Strobo"), 0.0, st.depth - 0.4, st.truss_back, true, Group::Strobes, 30.0, a);
    for (i, x) in [-1.0f32, 1.0].iter().enumerate() {
        let a = next(4);
        inst(p, "bar-rgb-4", &crate::i18n::t(&format!("Barra LED {}", i + 1)), *x, st.depth - 0.2, 0.1, false, Group::Bars, 80.0, a);
    }
}
