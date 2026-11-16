//! Drawing helpers shared by the Show timeline and the analysis editor:
//! bar/beat grid, waveforms, resizable lanes.

use crate::model::*;
use egui::{pos2, Color32, Painter, Stroke};

/// Waveform resolution (peaks per second) computed when the audio is loaded.
pub const PEAK_FPS: f64 = 100.0;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Lane {
    Sections,
    Cues,
    Hits,
    Energy,
    /// Any audio track: all tracks share the same height.
    Track,
}

pub fn lane_height(v: &mut ViewSettings, l: Lane) -> &mut f32 {
    match l {
        Lane::Sections => &mut v.sec_h,
        Lane::Cues => &mut v.cue_h,
        Lane::Hits => &mut v.hit_h,
        Lane::Energy => &mut v.energy_h,
        Lane::Track => &mut v.track_h,
    }
}

pub fn lane_limits(l: Lane) -> (f32, f32) {
    match l {
        Lane::Sections | Lane::Cues => (18.0, 90.0),
        Lane::Hits => (16.0, 80.0),
        Lane::Energy => (20.0, 220.0),
        Lane::Track => (8.0, 160.0),
    }
}

pub fn role_color(r: TrackRole) -> Color32 {
    match r {
        TrackRole::Drums | TrackRole::Kick | TrackRole::Snare => Color32::from_rgb(230, 120, 60),
        TrackRole::Bass => Color32::from_rgb(200, 80, 200),
        TrackRole::Guitar => Color32::from_rgb(230, 200, 60),
        TrackRole::Keys => Color32::from_rgb(80, 180, 230),
        TrackRole::Vocals => Color32::from_rgb(110, 220, 120),
        TrackRole::Click => Color32::from_rgb(120, 120, 120),
        _ => Color32::from_rgb(160, 160, 190),
    }
}

/// Bar lines, beat lines and bar numbers. `label_y` is where numbers go.
#[allow(clippy::too_many_arguments)]
pub fn draw_grid(
    p: &Painter,
    ana: &Analysis,
    start: f64,
    view_len: f64,
    pps: f32,
    tx: &dyn Fn(f64) -> f32,
    label_y: f32,
    y_top: f32,
    y_end: f32,
) {
    let spb = ana.spb();
    let bars = ana.bar_starts();
    let bar_px = (ana.bar_len() as f32 * pps).max(1.0);
    let label_every = ((48.0 / bar_px).ceil() as usize).max(1);
    let show_beats = pps * spb as f32 > 12.0;
    let anchors = ana.anchors();
    let a = start - ana.bar_len();
    let z = start + view_len + ana.bar_len();
    // the 4-bar phrase count restarts at every downbeat (first and extra ones)
    let mut local = 0usize;
    for (i, &t) in bars.iter().enumerate() {
        let is_anchor = anchors.iter().any(|x| (x - t).abs() < 1e-3);
        if is_anchor {
            local = 0;
        }
        let phrase = local % 4 == 0;
        local += 1;
        if t < a || t > z {
            continue;
        }
        let x = tx(t);
        let n = i + 1;
        let (w, col) = if is_anchor {
            (2.0, Color32::from_rgb(110, 100, 80))
        } else if phrase {
            (1.2, Color32::from_rgb(70, 70, 90))
        } else {
            (0.5, Color32::from_rgb(38, 38, 50))
        };
        p.line_segment([pos2(x, y_top), pos2(x, y_end)], Stroke::new(w, col));
        if (n - 1) % label_every == 0 || is_anchor {
            p.text(pos2(x + 2.0, label_y), egui::Align2::LEFT_TOP, format!("{n}"), egui::FontId::monospace(10.0), Color32::from_rgb(150, 150, 165));
        }
        if show_beats {
            let next = bars.get(i + 1).copied().unwrap_or(ana.duration);
            let mut b = t + spb;
            while b < next - 0.01 {
                let xb = tx(b);
                p.line_segment([pos2(xb, y_top + 6.0), pos2(xb, y_end)], Stroke::new(0.5, Color32::from_rgb(27, 27, 36)));
                b += spb;
            }
        }
    }
    // extra downbeats are highlighted
    for &t in anchors.iter().skip(1) {
        if t >= a && t <= z {
            let x = tx(t);
            p.line_segment([pos2(x, y_top), pos2(x, y_end)], Stroke::new(1.0, Color32::from_rgba_unmultiplied(255, 170, 60, 110)));
        }
    }
}

/// Draw a track: real waveform peaks if available, otherwise the loudness envelope.
#[allow(clippy::too_many_arguments)]
pub fn draw_wave(
    p: &Painter,
    x0: f32,
    right: f32,
    xt: &dyn Fn(f32) -> f64,
    dur: f64,
    y: f32,
    h: f32,
    peaks: Option<&[f32]>,
    env: &[f32],
    col: Color32,
) {
    let mid = y + h / 2.0;
    p.line_segment([pos2(x0, mid), pos2(right, mid)], Stroke::new(0.5, col.gamma_multiply(0.25)));
    let step = 1.0f32;
    let mut x = x0;
    while x < right {
        let t0 = xt(x);
        let t1 = xt(x + step);
        if t1 >= 0.0 && t0 <= dur {
            let v = match peaks {
                Some(pk) if !pk.is_empty() => {
                    let a = (t0.max(0.0) * PEAK_FPS) as usize;
                    let b = ((t1 * PEAK_FPS).ceil() as usize).max(a + 1).min(pk.len());
                    if a < b {
                        pk[a..b].iter().cloned().fold(0f32, f32::max)
                    } else {
                        0.0
                    }
                }
                _ => env.get((t0.max(0.0) * ENV_FPS) as usize).copied().unwrap_or(0.0) * 0.8,
            };
            // square root so quiet parts stay visible
            let v = v.clamp(0.0, 1.0).sqrt();
            if v > 0.01 {
                let hh = v * (h - 2.0) * 0.5;
                p.line_segment([pos2(x, mid - hh), pos2(x, mid + hh)], Stroke::new(step + 0.2, col.gamma_multiply(0.85)));
            }
        }
        x += step;
    }
}

/// Peaks (max |sample|) at PEAK_FPS of a mono signal.
pub fn peaks(mono: &[f32], sr: u32) -> Vec<f32> {
    let win = ((sr as f64 / PEAK_FPS) as usize).max(1);
    mono.chunks(win).map(|c| c.iter().fold(0f32, |m, x| m.max(x.abs()))).collect()
}
