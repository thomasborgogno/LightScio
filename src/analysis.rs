//! Audio decoding and music analysis: tempo, downbeats, song sections,
//! intensity, drum hits, accents (stacchi) and stops.

use crate::model::*;
use rustfft::{num_complex::Complex, FftPlanner};
use std::sync::{Arc, Mutex};

pub const AN_SR: f64 = 11025.0;
pub const FFT_N: usize = 1024;
pub const HOP: usize = 256;

#[derive(Default, Clone)]
pub struct Progress {
    pub frac: f32,
    pub msg: String,
}
pub type ProgressHandle = Arc<Mutex<Progress>>;

fn set_progress(p: &ProgressHandle, frac: f32, msg: &str) {
    if let Ok(mut g) = p.lock() {
        g.frac = frac;
        g.msg = msg.to_string();
    }
}

/// Stereo interleaved audio used for the preview player.
pub struct AudioBuf {
    pub sr: u32,
    pub data: Vec<f32>,
}

impl AudioBuf {
    pub fn duration(&self) -> f64 {
        self.data.len() as f64 / 2.0 / self.sr.max(1) as f64
    }
}

pub struct Decoded {
    pub sr: u32,
    pub left: Vec<f32>,
    pub right: Vec<f32>,
}

/// Decode an MP3 / WAV / FLAC / OGG file to stereo f32.
pub fn decode(path: &str) -> Result<Decoded, String> {
    use symphonia::core::audio::SampleBuffer;
    use symphonia::core::codecs::DecoderOptions;
    use symphonia::core::errors::Error;
    use symphonia::core::formats::FormatOptions;
    use symphonia::core::io::MediaSourceStream;
    use symphonia::core::meta::MetadataOptions;
    use symphonia::core::probe::Hint;

    if let Some(name) = path.strip_prefix("demo:") {
        let (sr, mono) = crate::demo::synth_track(name);
        return Ok(Decoded { sr, left: mono.clone(), right: mono });
    }

    let file = std::fs::File::open(path).map_err(|e| format!("{path}: {e}"))?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(ext) = std::path::Path::new(path).extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }
    let probed = symphonia::default::get_probe()
        .format(&hint, mss, &FormatOptions::default(), &MetadataOptions::default())
        .map_err(|e| format!("{path}: formato non riconosciuto ({e})"))?;
    let mut format = probed.format;
    let track = format.default_track().ok_or_else(|| format!("{path}: nessuna traccia audio"))?;
    let track_id = track.id;
    let mut sr = track.codec_params.sample_rate.unwrap_or(44100);
    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())
        .map_err(|e| format!("{path}: codec non supportato ({e})"))?;

    let mut left = Vec::new();
    let mut right = Vec::new();
    let mut sbuf: Option<SampleBuffer<f32>> = None;
    loop {
        let packet = match format.next_packet() {
            Ok(p) => p,
            Err(Error::IoError(_)) => break,
            Err(Error::ResetRequired) => break,
            Err(_) => break,
        };
        if packet.track_id() != track_id {
            continue;
        }
        match decoder.decode(&packet) {
            Ok(decoded) => {
                let spec = *decoded.spec();
                sr = spec.rate;
                let chn = spec.channels.count().max(1);
                let need = decoded.capacity() as u64;
                if sbuf.as_ref().map(|b| b.capacity() < need as usize * chn).unwrap_or(true) {
                    sbuf = Some(SampleBuffer::<f32>::new(need, spec));
                }
                let b = sbuf.as_mut().unwrap();
                b.copy_interleaved_ref(decoded);
                for fr in b.samples().chunks(chn) {
                    let l = fr[0];
                    let r = if chn > 1 { fr[1] } else { fr[0] };
                    left.push(l);
                    right.push(r);
                }
            }
            Err(Error::DecodeError(_)) => continue,
            Err(_) => break,
        }
    }
    if left.is_empty() {
        return Err(format!("{path}: file vuoto o illeggibile"));
    }
    Ok(Decoded { sr, left, right })
}

/// Per-track features at AN_SR / HOP frames per second.
pub struct Feat {
    pub role: TrackRole,
    pub rms: Vec<f32>,
    pub low: Vec<f32>,
    pub mid: Vec<f32>,
    pub high: Vec<f32>,
    pub flux: Vec<f32>,
    pub flux_low: Vec<f32>,
    pub flux_mid: Vec<f32>,
}

pub fn fps() -> f64 {
    AN_SR / HOP as f64
}

/// Box-filter resample of a mono signal to AN_SR.
fn to_analysis_rate(mono: &[f32], sr: u32) -> Vec<f32> {
    let ratio = sr as f64 / AN_SR;
    let n = (mono.len() as f64 / ratio) as usize;
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let a = (i as f64 * ratio) as usize;
        let b = (((i + 1) as f64 * ratio) as usize).min(mono.len()).max(a + 1);
        let s: f32 = mono[a..b.min(mono.len())].iter().sum();
        out.push(s / (b - a) as f32);
    }
    out
}

pub fn features(mono_sr: u32, mono: &[f32], role: TrackRole) -> Feat {
    let x = to_analysis_rate(mono, mono_sr);
    let mut planner = FftPlanner::<f32>::new();
    let fft = planner.plan_fft_forward(FFT_N);
    let win: Vec<f32> =
        (0..FFT_N).map(|i| 0.5 - 0.5 * (2.0 * std::f32::consts::PI * i as f32 / FFT_N as f32).cos()).collect();
    let frames = if x.len() > FFT_N { (x.len() - FFT_N) / HOP + 1 } else { 1 };
    let bin_hz = AN_SR as f32 / FFT_N as f32;
    let lo_end = (150.0 / bin_hz) as usize;
    let mid_end = (2000.0 / bin_hz) as usize;
    let half = FFT_N / 2;
    let mut f = Feat {
        role,
        rms: Vec::with_capacity(frames),
        low: Vec::with_capacity(frames),
        mid: Vec::with_capacity(frames),
        high: Vec::with_capacity(frames),
        flux: Vec::with_capacity(frames),
        flux_low: Vec::with_capacity(frames),
        flux_mid: Vec::with_capacity(frames),
    };
    let mut prev = vec![0f32; half];
    let mut buf = vec![Complex::new(0f32, 0f32); FFT_N];
    for fr in 0..frames {
        let start = fr * HOP;
        let mut sq = 0f32;
        for i in 0..FFT_N {
            let v = x.get(start + i).copied().unwrap_or(0.0);
            sq += v * v;
            buf[i] = Complex::new(v * win[i], 0.0);
        }
        fft.process(&mut buf);
        let (mut lo, mut mi, mut hi) = (0f32, 0f32, 0f32);
        let (mut fl, mut flo, mut fmi) = (0f32, 0f32, 0f32);
        for k in 1..half {
            let m = buf[k].norm() / FFT_N as f32;
            let e = m * m;
            let lm = (1.0 + 1000.0 * m).ln();
            let d = (lm - prev[k]).max(0.0);
            prev[k] = lm;
            fl += d;
            if k < lo_end {
                lo += e;
                flo += d;
            } else if k < mid_end {
                mi += e;
                fmi += d;
            } else {
                hi += e;
            }
        }
        f.rms.push((sq / FFT_N as f32).sqrt());
        f.low.push(lo);
        f.mid.push(mi);
        f.high.push(hi);
        f.flux.push(fl);
        f.flux_low.push(flo);
        f.flux_mid.push(fmi);
    }
    f
}

// ---------------------------------------------------------------- helpers

fn percentile(v: &[f32], p: f32) -> f32 {
    if v.is_empty() {
        return 0.0;
    }
    let mut s: Vec<f32> = v.iter().copied().filter(|x| x.is_finite()).collect();
    if s.is_empty() {
        return 0.0;
    }
    s.sort_by(|a, b| a.total_cmp(b));
    let i = ((s.len() - 1) as f32 * p.clamp(0.0, 1.0)) as usize;
    s[i]
}

fn normalize(v: &[f32], p: f32) -> Vec<f32> {
    let m = percentile(v, p).max(1e-6);
    v.iter().map(|x| (x / m).min(1.5)).collect()
}

fn moving_mean(v: &[f32], half: usize) -> Vec<f32> {
    let n = v.len();
    let mut pre = vec![0f64; n + 1];
    for i in 0..n {
        pre[i + 1] = pre[i] + v[i] as f64;
    }
    (0..n)
        .map(|i| {
            let a = i.saturating_sub(half);
            let b = (i + half + 1).min(n);
            ((pre[b] - pre[a]) / (b - a).max(1) as f64) as f32
        })
        .collect()
}

fn interp(v: &[f32], pos: f64) -> f32 {
    if pos < 0.0 {
        return 0.0;
    }
    let i = pos.floor() as usize;
    if i + 1 >= v.len() {
        return v.last().copied().unwrap_or(0.0) * if i < v.len() { 1.0 } else { 0.0 };
    }
    let fr = (pos - i as f64) as f32;
    v[i] * (1.0 - fr) + v[i + 1] * fr
}

fn peak_pick(o: &[f32], fps: f64, sens: f32, min_gap: f64) -> Vec<f64> {
    let mean = moving_mean(o, (0.4 * fps) as usize);
    let mut out: Vec<f64> = vec![];
    let w = 3usize;
    for i in 0..o.len() {
        let thr = sens * mean[i] + 0.08;
        if o[i] <= thr {
            continue;
        }
        let a = i.saturating_sub(w);
        let b = (i + w + 1).min(o.len());
        if o[a..b].iter().any(|&x| x > o[i]) {
            continue;
        }
        let t = i as f64 / fps;
        if let Some(&last) = out.last() {
            if t - last < min_gap {
                continue;
            }
        }
        out.push(t);
    }
    out
}

fn dist(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| (x - y) * (x - y)).sum::<f32>().sqrt()
}

fn mean_vec(rows: &[Vec<f32>]) -> Vec<f32> {
    if rows.is_empty() {
        return vec![];
    }
    let n = rows[0].len();
    let mut m = vec![0f32; n];
    for r in rows {
        for (i, x) in r.iter().enumerate() {
            m[i] += x;
        }
    }
    for x in &mut m {
        *x /= rows.len() as f32;
    }
    m
}

// ---------------------------------------------------------------- tempo

fn estimate_tempo(o: &[f32], fps: f64) -> (f64, f64) {
    let n = o.len();
    if n < (fps * 4.0) as usize {
        return (120.0, 0.0);
    }
    let min_lag = (fps * 60.0 / 200.0) as usize;
    let max_lag = ((fps * 60.0 / 60.0) as usize).min(n / 2);
    let mut best = (0f64, 120.0f64);
    for lag in min_lag..=max_lag {
        let mut s = 0f64;
        for i in 0..n - lag {
            s += (o[i] * o[i + lag]) as f64;
        }
        s /= (n - lag) as f64;
        let bpm = 60.0 * fps / lag as f64;
        let w = (-0.5 * ((bpm / 120.0).log2() / 0.9).powi(2)).exp();
        let sc = s * w;
        if sc > best.0 {
            best = (sc, bpm);
        }
    }
    let bpm0 = best.1;
    // refine with phase-histogram comb
    let comb = |bpm: f64| -> (f64, f64) {
        let period = 60.0 / bpm;
        let nb = 40usize;
        let mut h = vec![0f64; nb];
        for (i, &v) in o.iter().enumerate() {
            let t = i as f64 / fps;
            let ph = (t / period).fract();
            h[((ph * nb as f64) as usize).min(nb - 1)] += v as f64;
        }
        let mut bi = 0;
        let mut bs = f64::MIN;
        for b in 0..nb {
            let s = h[(b + nb - 1) % nb] + 2.0 * h[b] + h[(b + 1) % nb];
            if s > bs {
                bs = s;
                bi = b;
            }
        }
        (bs, (bi as f64 + 0.5) / nb as f64 * period)
    };
    let mut bb = (f64::MIN, bpm0);
    let mut x = bpm0 - 3.0;
    while x <= bpm0 + 3.0 {
        let (s, _) = comb(x);
        if s > bb.0 {
            bb = (s, x);
        }
        x += 0.02;
    }
    let mut bpm = bb.1;
    if (bpm - bpm.round()).abs() < 0.25 {
        bpm = bpm.round();
    }
    // precise phase scan
    let period = 60.0 / bpm;
    let beats = (n as f64 / fps / period) as usize;
    let mut best_ph = (f64::MIN, 0.0);
    let mut ph = 0.0;
    while ph < period {
        let mut s = 0f64;
        for k in 0..beats {
            s += interp(o, (ph + k as f64 * period) * fps) as f64;
        }
        if s > best_ph.0 {
            best_ph = (s, ph);
        }
        ph += 0.004;
    }
    (bpm, best_ph.1)
}

// ---------------------------------------------------------------- main analysis

pub struct AnalysisOut {
    pub analysis: Analysis,
    pub sections: Vec<Section>,
    pub track_envs: Vec<Vec<f32>>,
}

pub fn analyze(feats: &[Feat], duration: f64, bpb: u32) -> AnalysisOut {
    let fps = fps();
    let n = feats.iter().map(|f| f.rms.len()).max().unwrap_or(0);
    let bpb = bpb.max(1);
    let at = |v: &Vec<f32>, i: usize| v.get(i).copied().unwrap_or(0.0);

    let musical: Vec<usize> =
        (0..feats.len()).filter(|&i| !matches!(feats[i].role, TrackRole::Click | TrackRole::Ignore)).collect();
    let clicks: Vec<usize> = (0..feats.len()).filter(|&i| feats[i].role == TrackRole::Click).collect();
    let drums: Vec<usize> = (0..feats.len()).filter(|&i| feats[i].role.is_drum()).collect();

    // normalized per-track curves
    let rms_n: Vec<Vec<f32>> = feats.iter().map(|f| normalize(&f.rms, 0.98)).collect();
    let flux_n: Vec<Vec<f32>> = feats.iter().map(|f| normalize(&f.flux, 0.99)).collect();

    // global energy
    let mut e = vec![0f32; n];
    for &t in &musical {
        for i in 0..n {
            e[i] += at(&feats[t].rms, i);
        }
    }
    let e = normalize(&e, 0.98).into_iter().map(|x| x.min(1.0)).collect::<Vec<_>>();
    let e_s = moving_mean(&e, 3);

    // onset envelope
    let mut o = vec![0f32; n];
    for &t in &musical {
        let w = if feats[t].role.is_drum() { 2.0 } else { 1.0 };
        for i in 0..n {
            o[i] += w * at(&flux_n[t], i);
        }
    }
    let om = moving_mean(&o, 10);
    let o2: Vec<f32> = (0..n).map(|i| (o[i] - om[i]).max(0.0)).collect();
    let o2 = normalize(&o2, 0.99);

    // tempo: prefer the click track
    let (tempo_env, used_click) = if !clicks.is_empty() {
        let mut c = vec![0f32; n];
        for &t in &clicks {
            for i in 0..n {
                c[i] += at(&flux_n[t], i);
            }
        }
        (c, true)
    } else {
        (o2.clone(), false)
    };
    let (bpm, phase) = estimate_tempo(&tempo_env, fps);
    let spb = 60.0 / bpm;

    // drum hits
    let kick_src: Vec<f32> = {
        let kicks: Vec<usize> = (0..feats.len()).filter(|&i| feats[i].role == TrackRole::Kick).collect();
        let src: Vec<usize> = if !kicks.is_empty() {
            kicks
        } else if !drums.is_empty() {
            drums.clone()
        } else {
            musical.clone()
        };
        let mut v = vec![0f32; n];
        for &t in &src {
            let fl = normalize(&feats[t].flux_low, 0.99);
            for i in 0..n {
                v[i] += at(&fl, i);
            }
        }
        normalize(&v, 0.99)
    };
    let snare_src: Option<Vec<f32>> = {
        let sn: Vec<usize> = (0..feats.len()).filter(|&i| feats[i].role == TrackRole::Snare).collect();
        let src = if !sn.is_empty() {
            sn
        } else {
            (0..feats.len()).filter(|&i| feats[i].role == TrackRole::Drums).collect()
        };
        if src.is_empty() {
            None
        } else {
            let mut v = vec![0f32; n];
            for &t in &src {
                let fm = normalize(&feats[t].flux_mid, 0.99);
                for i in 0..n {
                    v[i] += at(&fm, i);
                }
            }
            Some(normalize(&v, 0.99))
        }
    };
    let kicks = peak_pick(&kick_src, fps, 1.6, 0.12);
    let snares = match &snare_src {
        Some(s) => {
            let raw = peak_pick(s, fps, 1.6, 0.12);
            // drop snare candidates that coincide with a kick
            raw.into_iter().filter(|t| !kicks.iter().any(|k| (k - t).abs() < 0.03)).collect()
        }
        None => vec![],
    };

    // downbeat: which beat of the bar has the strongest kicks
    let nbeats = ((duration - phase) / spb).max(0.0) as usize;
    let mut best_p = (f64::MIN, 0usize);
    for p in 0..bpb as usize {
        let mut s = 0f64;
        for k in (p..nbeats).step_by(bpb as usize) {
            let t = phase + k as f64 * spb;
            let i = (t * fps) as usize;
            let a = i.saturating_sub(2);
            let b = (i + 3).min(n);
            if a < b {
                s += kick_src[a..b].iter().cloned().fold(0f32, f32::max) as f64;
                s += 0.5 * e_s.get(b.min(n.saturating_sub(1))).copied().unwrap_or(0.0) as f64
                    - 0.5 * e_s.get(a.saturating_sub(4)).copied().unwrap_or(0.0) as f64;
            }
        }
        if s > best_p.0 {
            best_p = (s, p);
        }
    }
    let bar_len = spb * bpb as f64;
    let mut offset = (phase + best_p.1 as f64 * spb) % bar_len;
    if offset > bar_len - 0.08 {
        offset = 0.0;
    }

    // ---- per-bar features
    let n_bars = (((duration - offset) / bar_len).ceil() as usize).max(1);
    let frame_range = |t0: f64, t1: f64| -> (usize, usize) {
        let a = ((t0.max(0.0)) * fps) as usize;
        let b = ((t1 * fps) as usize).min(n).max(a + 1);
        (a.min(n.saturating_sub(1)), b)
    };
    let mut bar_feat: Vec<Vec<f32>> = Vec::with_capacity(n_bars);
    let mut bar_energy: Vec<f32> = Vec::with_capacity(n_bars);
    for b in 0..n_bars {
        let t0 = if b == 0 { 0.0 } else { offset + b as f64 * bar_len };
        let t1 = offset + (b + 1) as f64 * bar_len;
        let (a, z) = frame_range(t0, t1);
        let len = (z - a).max(1) as f32;
        let mut v = vec![];
        for &t in &musical {
            let s: f32 = (a..z).map(|i| at(&rms_n[t], i)).sum::<f32>() / len;
            v.push(s.min(1.2));
        }
        let (mut lo, mut mi, mut hi) = (0f32, 0f32, 0f32);
        for &t in &musical {
            for i in a..z {
                lo += at(&feats[t].low, i);
                mi += at(&feats[t].mid, i);
                hi += at(&feats[t].high, i);
            }
        }
        let tot = (lo + mi + hi).max(1e-9);
        let en: f32 = (a..z).map(|i| e[i.min(n.saturating_sub(1))]).sum::<f32>() / len;
        let dens: f32 = (a..z).map(|i| at(&o2, i)).sum::<f32>() / len;
        v.push(lo / tot);
        v.push(mi / tot);
        v.push(hi / tot);
        v.push(en * 2.0);
        v.push(dens * 1.5);
        bar_feat.push(v);
        bar_energy.push(en);
    }

    // ---- novelty & boundaries
    let mut nov = vec![0f32; n_bars + 1];
    for b in 1..n_bars {
        let mut s = 0f32;
        for (k, w) in [(2usize, 0.6f32), (4, 1.0), (8, 0.7)] {
            let a = b.saturating_sub(k);
            let z = (b + k).min(n_bars);
            if b - a >= 1 && z - b >= 1 {
                let m1 = mean_vec(&bar_feat[a..b]);
                let m2 = mean_vec(&bar_feat[b..z]);
                s += w * dist(&m1, &m2);
            }
        }
        if b % 4 == 0 {
            s *= 1.15;
        }
        if b % 8 == 0 {
            s *= 1.1;
        }
        nov[b] = s;
    }
    let inner: Vec<f32> = nov[1..n_bars.max(2)].to_vec();
    let mean_n = inner.iter().sum::<f32>() / inner.len().max(1) as f32;
    let std_n = (inner.iter().map(|x| (x - mean_n).powi(2)).sum::<f32>() / inner.len().max(1) as f32).sqrt();
    let thr = mean_n + 0.35 * std_n;
    let mut cands: Vec<usize> = (1..n_bars)
        .filter(|&b| nov[b] >= nov[b - 1] && nov[b] >= nov[(b + 1).min(n_bars)] && nov[b] > thr)
        .collect();
    cands.sort_by(|a, b| nov[*b].total_cmp(&nov[*a]));
    let max_sections = (n_bars / 4).clamp(1, 16);
    let mut bounds: Vec<usize> = vec![];
    for c in cands {
        if bounds.len() + 1 >= max_sections {
            break;
        }
        let strong = nov[c] > thr + 1.5 * std_n;
        let min_gap = if strong { 2 } else { 4 };
        if c < min_gap.min(2) || n_bars - c < 2 {
            continue;
        }
        if bounds.iter().all(|&x| (x as i64 - c as i64).unsigned_abs() as usize >= min_gap) {
            bounds.push(c);
        }
    }
    bounds.push(0);
    bounds.push(n_bars);
    bounds.sort();
    bounds.dedup();

    // ---- segments
    struct Seg {
        a: usize,
        z: usize,
        feat: Vec<f32>,
        energy: f32,
        dens: f32,
        cluster: usize,
    }
    let beat_hits = |t0: f64, t1: f64| -> f32 {
        let k = kicks.iter().filter(|&&t| t >= t0 && t < t1).count();
        let s = snares.iter().filter(|&&t| t >= t0 && t < t1).count();
        let beats = ((t1 - t0) / spb).max(1.0);
        ((k + s) as f64 / beats) as f32
    };
    let bar_t = |b: usize| -> f64 {
        if b == 0 {
            0.0
        } else if b >= n_bars {
            duration
        } else {
            offset + b as f64 * bar_len
        }
    };
    let mut segs: Vec<Seg> = bounds
        .windows(2)
        .map(|w| {
            let (a, z) = (w[0], w[1]);
            let feat = mean_vec(&bar_feat[a..z]);
            let energy = bar_energy[a..z].iter().sum::<f32>() / (z - a) as f32;
            let dens = beat_hits(bar_t(a), bar_t(z));
            Seg { a, z, feat, energy, dens, cluster: usize::MAX }
        })
        .collect();

    // clustering
    let mut pd = vec![];
    for i in 0..segs.len() {
        for j in i + 1..segs.len() {
            pd.push(dist(&segs[i].feat, &segs[j].feat));
        }
    }
    let cthr = if pd.len() >= 3 { percentile(&pd, 0.5) * 0.6 } else { 0.25 };
    let mut centroids: Vec<(Vec<f32>, usize)> = vec![];
    for s in segs.iter_mut() {
        let mut best = (f32::MAX, usize::MAX);
        for (ci, (c, _)) in centroids.iter().enumerate() {
            // never group parts with clearly different loudness
            let ce = c[c.len() - 2] / 2.0;
            let ratio = (s.energy.max(1e-3) / ce.max(1e-3)).min(ce.max(1e-3) / s.energy.max(1e-3));
            if ratio < 0.82 {
                continue;
            }
            let d = dist(&s.feat, c);
            if d < best.0 {
                best = (d, ci);
            }
        }
        if best.0 < cthr {
            s.cluster = best.1;
            let (c, cnt) = &mut centroids[best.1];
            for (i, x) in s.feat.iter().enumerate() {
                c[i] = (c[i] * *cnt as f32 + x) / (*cnt + 1) as f32;
            }
            *cnt += 1;
        } else {
            s.cluster = centroids.len();
            centroids.push((s.feat.clone(), 1));
        }
    }

    // ---- intensity
    let max_e = segs.iter().map(|s| s.energy).fold(0.01f32, f32::max);
    let intens: Vec<f32> = segs
        .iter()
        .map(|s| {
            let rel = (s.energy / max_e).clamp(0.0, 1.0);
            let dens = (s.dens / 2.0).clamp(0.0, 1.0);
            (rel.powf(1.3) * (0.55 + 0.45 * dens)).clamp(0.02, 1.0)
        })
        .collect();

    // ---- labels
    let nseg = segs.len();
    let mut kinds = vec![SectionKind::Other; nseg];
    let count = |c: usize, segs: &Vec<Seg>| segs.iter().filter(|s| s.cluster == c).count();
    let med_e = {
        let v: Vec<f32> = segs.iter().map(|s| s.energy).collect();
        percentile(&v, 0.5)
    };
    // chorus = repeating cluster with highest mean energy
    let mut cl_energy: Vec<(usize, f32, usize)> = vec![];
    for c in 0..centroids.len() {
        let members: Vec<&Seg> = segs.iter().filter(|s| s.cluster == c).collect();
        let me = members.iter().map(|s| s.energy).sum::<f32>() / members.len().max(1) as f32;
        cl_energy.push((c, me, members.len()));
    }
    let repeating: Vec<&(usize, f32, usize)> = cl_energy.iter().filter(|x| x.2 >= 2).collect();
    let chorus_c = repeating.iter().max_by(|a, b| a.1.total_cmp(&b.1)).map(|x| x.0);
    let verse_c = repeating
        .iter()
        .filter(|x| Some(x.0) != chorus_c && x.1 < cl_energy.iter().find(|y| Some(y.0) == chorus_c).map(|y| y.1).unwrap_or(1.0))
        .min_by_key(|x| segs.iter().position(|s| s.cluster == x.0).unwrap_or(999))
        .map(|x| x.0);
    for i in 0..nseg {
        let s = &segs[i];
        kinds[i] = if Some(s.cluster) == chorus_c {
            SectionKind::Chorus
        } else if Some(s.cluster) == verse_c {
            SectionKind::Verse
        } else if count(s.cluster, &segs) >= 2 {
            // repeating, neither verse nor chorus: pre-chorus if followed by a chorus
            if i + 1 < nseg && Some(segs[i + 1].cluster) == chorus_c {
                SectionKind::PreChorus
            } else {
                SectionKind::Verse
            }
        } else if s.energy < med_e * 0.45 {
            SectionKind::Break
        } else if i + 1 < nseg && Some(segs[i + 1].cluster) == chorus_c && s.z - s.a <= 4 {
            SectionKind::PreChorus
        } else if s.energy >= med_e && i as f32 > nseg as f32 * 0.5 {
            SectionKind::Solo
        } else {
            SectionKind::Bridge
        };
    }
    if chorus_c.is_none() {
        // no repetitions: label by energy
        for i in 0..nseg {
            let r = segs[i].energy / max_e;
            kinds[i] = if r > 0.8 {
                SectionKind::Chorus
            } else if r > 0.45 {
                SectionKind::Verse
            } else {
                SectionKind::Break
            };
        }
    }
    if nseg >= 3 {
        if segs[0].energy < med_e * 0.9 || count(segs[0].cluster, &segs) == 1 {
            kinds[0] = SectionKind::Intro;
        }
        let l = nseg - 1;
        if count(segs[l].cluster, &segs) == 1 || segs[l].energy < segs[l - 1].energy * 0.8 {
            kinds[l] = SectionKind::Outro;
        }
    } else if nseg == 2 {
        kinds[0] = SectionKind::Intro;
    }
    let mut sections: Vec<Section> = (0..nseg)
        .map(|i| Section { start: bar_t(segs[i].a), end: bar_t(segs[i].z), kind: kinds[i], label: String::new(), intensity: intens[i] })
        .collect();

    // ---- stops (stacchi): sudden near-silence after loud playing
    let mut stops = vec![];
    let mut accents: Vec<(f64, f32)> = vec![];
    let ref_e = moving_mean(&e_s, (fps * 1.0) as usize);
    let mut i = (fps * 2.0) as usize;
    let tail = n.saturating_sub((fps * 3.0) as usize);
    while i < tail {
        let r = ref_e[i.saturating_sub((fps * 1.0) as usize)];
        if r > 0.25 && e_s[i] < 0.12 * r && e_s[i.saturating_sub((0.7 * fps) as usize)] > 0.3 * r {
            let mut j = i;
            while j < n && e_s[j] < 0.3 * r {
                j += 1;
            }
            let dur = (j - i) as f64 / fps;
            if (0.15..=4.0).contains(&dur) && j < n {
                stops.push((i as f64 / fps, dur));
                let back = e_s[(j + 6).min(n - 1)];
                accents.push((j as f64 / fps, (back / r).clamp(0.5, 1.0)));
            }
            i = j + 1;
        } else {
            i += 1;
        }
    }

    // a stop lasting about a bar or more becomes its own "break" part
    for &(t, d) in &stops {
        let s0 = offset + ((t - offset) / bar_len).floor() * bar_len;
        let s1 = offset + ((t + d - offset) / bar_len).round() * bar_len;
        if s1 - s0 < bar_len * 0.95 {
            continue;
        }
        let Some(k) = sections.iter().position(|s| s0 >= s.start - 0.01 && s0 < s.end - 0.01) else { continue };
        let orig = sections[k].clone();
        let s1 = s1.min(orig.end);
        let mut parts = vec![];
        if s0 - orig.start > bar_len * 0.5 {
            parts.push(Section { end: s0, ..orig.clone() });
        }
        parts.push(Section { start: s0.max(orig.start), end: s1, kind: SectionKind::Break, label: String::new(), intensity: 0.15 });
        if orig.end - s1 > bar_len * 0.5 {
            parts.push(Section { start: s1, ..orig.clone() });
        }
        sections.splice(k..k + 1, parts);
    }
    // a quiet part right before the ending belongs to the ending
    let ns = sections.len();
    if ns >= 3 && sections[ns - 1].kind == SectionKind::Outro {
        let p = &sections[ns - 2];
        if p.kind != SectionKind::Chorus && p.intensity < 0.25 && p.intensity < sections[ns - 3].intensity * 0.5 {
            let end = sections[ns - 1].end;
            sections[ns - 2].kind = SectionKind::Outro;
            sections[ns - 2].end = end;
            sections.pop();
        }
    }
    // breaks are calmer; numbering
    let mut counters = std::collections::HashMap::new();
    let kinds_now: Vec<SectionKind> = sections.iter().map(|s| s.kind).collect();
    for s in &mut sections {
        if s.kind == SectionKind::Break {
            s.intensity = s.intensity.min(0.3);
        }
        let c = counters.entry(s.kind).or_insert(0);
        *c += 1;
        let total = kinds_now.iter().filter(|x| **x == s.kind).count();
        s.label = if total > 1 { format!("{} {}", s.kind.label(), c) } else { s.kind.label().to_string() };
    }

    // ---- unison accents: many instruments hitting together with an energy jump
    let mut cand: Vec<(f64, f32)> = vec![];
    if musical.len() >= 2 {
        let p95 = percentile(&o2, 0.97);
        for i in 15..n.saturating_sub(8) {
            if o2[i] < p95 {
                continue;
            }
            let a = i.saturating_sub(5);
            let z = (i + 6).min(n);
            if o2[a..z].iter().any(|&x| x > o2[i]) {
                continue;
            }
            let mut hitters = 0;
            let mut active = 0;
            for &t in &musical {
                let act = (i.saturating_sub(10)..(i + 10).min(n)).any(|k| at(&rms_n[t], k) > 0.08);
                if !act {
                    continue;
                }
                active += 1;
                if (i.saturating_sub(1)..=(i + 1)).any(|k| at(&flux_n[t], k) > 0.35) {
                    hitters += 1;
                }
            }
            if active == 0 {
                continue;
            }
            let ratio = hitters as f32 / active as f32;
            let before: f32 = e_s[i - 15..i - 3].iter().sum::<f32>() / 12.0;
            let after: f32 = e_s[i..i + 6].iter().sum::<f32>() / 6.0;
            let jump = after / before.max(0.02);
            if hitters >= 2 && ratio >= 0.6 && (jump > 1.3 || o2[i] > 1.0) {
                cand.push((i as f64 / fps, (ratio * o2[i].min(1.2) * jump.min(3.0) / 3.0).clamp(0.1, 1.0)));
            }
        }
    }
    cand.sort_by(|a, b| b.1.total_cmp(&a.1));
    let max_acc = ((duration / 12.0) as usize).max(2);
    for c in cand {
        if accents.len() >= max_acc + stops.len() {
            break;
        }
        if accents.iter().all(|a| (a.0 - c.0).abs() > 1.5) {
            accents.push(c);
        }
    }
    accents.sort_by(|a, b| a.0.total_cmp(&b.0));

    // ---- character
    let total_beats = ((duration - offset) / spb).max(1.0);
    let kick_on_beat = {
        let mut hit = 0;
        let nb = total_beats as usize;
        for k in 0..nb {
            let t = offset + k as f64 * spb;
            if kicks.iter().any(|&x| (x - t).abs() < 0.06) {
                hit += 1;
            }
        }
        hit as f64 / nb.max(1) as f64
    };
    let dur_w: f64 = sections.iter().map(|s| s.end - s.start).sum::<f64>().max(1.0);
    let mean_i = (sections.iter().map(|s| s.intensity as f64 * (s.end - s.start)).sum::<f64>() / dur_w) as f32;
    let dens_all = ((kicks.len() + snares.len()) as f64 / total_beats) as f32;
    let character = if kick_on_beat > 0.8 && bpm >= 112.0 {
        "Dance / elettronico (cassa dritta)"
    } else if mean_i > 0.5 && dens_all > 0.9 {
        "Rock / energico"
    } else if mean_i < 0.3 || (bpm < 85.0 && dens_all < 0.7) {
        "Ballad / tranquillo"
    } else {
        "Pop / groove medio"
    }
    .to_string();

    // ---- envelopes for drawing
    let to_env = |v: &[f32]| -> Vec<f32> {
        let m = (duration * ENV_FPS) as usize;
        (0..m)
            .map(|k| {
                let a = (k as f64 / ENV_FPS * fps) as usize;
                let z = (((k + 1) as f64 / ENV_FPS * fps) as usize).max(a + 1);
                let mut s = 0f32;
                let mut c = 0;
                for i in a..z.min(v.len()) {
                    s = s.max(v[i]);
                    c += 1;
                }
                if c == 0 {
                    0.0
                } else {
                    s.min(1.0)
                }
            })
            .collect()
    };
    let track_envs: Vec<Vec<f32>> = rms_n.iter().map(|v| to_env(v)).collect();
    let env = to_env(&e);

    AnalysisOut {
        analysis: Analysis {
            bpm,
            offset,
            beats_per_bar: bpb,
            duration,
            env,
            kicks,
            snares,
            accents,
            stops,
            character,
            mean_intensity: mean_i,
            used_click,
            downbeats: vec![],
        },
        sections,
        track_envs,
    }
}

// ---------------------------------------------------------------- job

pub struct LoadOutput {
    pub audio: Arc<AudioBuf>,
    pub analysis: Option<AnalysisOut>,
    pub errors: Vec<String>,
    /// Waveform peaks per track (same order as the tracks; empty if unreadable).
    pub peaks: Vec<Vec<f32>>,
}

/// Decode all tracks, build the preview mix and (optionally) analyze.
pub fn load_song(tracks: &[TrackInfo], analyze_it: bool, bpb: u32, progress: &ProgressHandle) -> Result<LoadOutput, String> {
    let mut mix: Vec<f32> = vec![];
    let mut mix_sr: u32 = 0;
    let mut feats = vec![];
    let mut errors = vec![];
    let mut peaks_out: Vec<Vec<f32>> = vec![];
    let mut max_dur = 0f64;
    let nt = tracks.len().max(1);
    for (ti, t) in tracks.iter().enumerate() {
        set_progress(progress, ti as f32 / nt as f32 * 0.9, &format!("Leggo {}", t.name));
        let d = match decode(&t.path) {
            Ok(d) => d,
            Err(e) => {
                errors.push(e);
                peaks_out.push(vec![]);
                if analyze_it {
                    feats.push(Feat {
                        role: TrackRole::Ignore,
                        rms: vec![],
                        low: vec![],
                        mid: vec![],
                        high: vec![],
                        flux: vec![],
                        flux_low: vec![],
                        flux_mid: vec![],
                    });
                }
                continue;
            }
        };
        let dur = d.left.len() as f64 / d.sr as f64;
        max_dur = max_dur.max(dur);
        {
            let win = ((d.sr as f64 / crate::lanes::PEAK_FPS) as usize).max(1);
            let pk: Vec<f32> = d
                .left
                .chunks(win)
                .zip(d.right.chunks(win))
                .map(|(l, r)| l.iter().zip(r).fold(0f32, |m, (a, b)| m.max((0.5 * (a + b)).abs())))
                .collect();
            peaks_out.push(pk);
        }
        if t.role != TrackRole::Click && t.role != TrackRole::Ignore {
            if mix_sr == 0 {
                mix_sr = d.sr;
            }
            let ratio = d.sr as f64 / mix_sr as f64;
            let frames = (d.left.len() as f64 / ratio) as usize;
            if mix.len() < frames * 2 {
                mix.resize(frames * 2, 0.0);
            }
            for i in 0..frames {
                let p = i as f64 * ratio;
                let k = p as usize;
                let fr = (p - k as f64) as f32;
                let get = |v: &Vec<f32>| {
                    let a = v.get(k).copied().unwrap_or(0.0);
                    let b = v.get(k + 1).copied().unwrap_or(a);
                    a + (b - a) * fr
                };
                mix[i * 2] += get(&d.left);
                mix[i * 2 + 1] += get(&d.right);
            }
        }
        if analyze_it {
            set_progress(progress, (ti as f32 + 0.5) / nt as f32 * 0.9, &format!("Analizzo {}", t.name));
            let mono: Vec<f32> = d.left.iter().zip(&d.right).map(|(l, r)| 0.5 * (l + r)).collect();
            let sr = d.sr;
            drop(d);
            feats.push(features(sr, &mono, t.role));
        }
    }
    if mix_sr == 0 {
        mix_sr = 44100;
    }
    let peak = mix.iter().fold(0f32, |m, x| m.max(x.abs()));
    if peak > 0.98 {
        let g = 0.95 / peak;
        for x in &mut mix {
            *x *= g;
        }
    }
    let audio = Arc::new(AudioBuf { sr: mix_sr, data: mix });
    let analysis = if analyze_it {
        set_progress(progress, 0.92, "Riconosco tempo, parti e stacchi…");
        if max_dur < 1.0 {
            return Err("Nessuna traccia leggibile".into());
        }
        Some(analyze(&feats, max_dur, bpb))
    } else {
        None
    };
    set_progress(progress, 1.0, "Fatto");
    Ok(LoadOutput { audio, analysis, errors, peaks: peaks_out })
}
