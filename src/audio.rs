//! Preview audio player (stand-alone rehearsal on the PC). In the live
//! scenario the audio comes from the backing-track player, not from here.

use crate::analysis::AudioBuf;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

struct PlayerShared {
    buf: Mutex<Option<Arc<AudioBuf>>>,
    /// Position in buffer frames (f64 bits).
    pos: AtomicU64,
    playing: AtomicBool,
    volume: AtomicU32,
    /// Set by the audio thread when the output device stops working.
    dead: AtomicBool,
    /// Metronome clicks (time, is_downbeat) mixed into the preview when enabled.
    metro: Mutex<Option<Arc<Vec<(f64, bool)>>>>,
    metro_on: AtomicBool,
}

pub struct Player {
    stream: Option<cpal::Stream>,
    sh: Arc<PlayerShared>,
    pub error: String,
}

impl Player {
    pub fn new() -> Self {
        Player {
            stream: None,
            sh: Arc::new(PlayerShared {
                buf: Mutex::new(None),
                pos: AtomicU64::new(0f64.to_bits()),
                playing: AtomicBool::new(false),
                volume: AtomicU32::new(0.8f32.to_bits()),
                dead: AtomicBool::new(false),
                metro: Mutex::new(None),
                metro_on: AtomicBool::new(false),
            }),
            error: String::new(),
        }
    }

    pub fn set_buffer(&mut self, b: Option<Arc<AudioBuf>>) {
        self.stop();
        if let Ok(mut g) = self.sh.buf.lock() {
            *g = b;
        }
    }
    pub fn has_buffer(&self) -> bool {
        self.sh.buf.lock().map(|g| g.is_some()).unwrap_or(false)
    }
    fn sr(&self) -> f64 {
        self.sh.buf.lock().ok().and_then(|g| g.as_ref().map(|b| b.sr as f64)).unwrap_or(44100.0)
    }
    pub fn position(&self) -> f64 {
        f64::from_bits(self.sh.pos.load(Ordering::Relaxed)) / self.sr()
    }
    pub fn is_playing(&self) -> bool {
        self.sh.playing.load(Ordering::Relaxed)
    }
    pub fn set_volume(&self, v: f32) {
        self.sh.volume.store(v.to_bits(), Ordering::Relaxed);
    }
    pub fn volume(&self) -> f32 {
        f32::from_bits(self.sh.volume.load(Ordering::Relaxed))
    }
    pub fn seek(&self, t: f64) {
        let frames = t.max(0.0) * self.sr();
        self.sh.pos.store(frames.to_bits(), Ordering::Relaxed);
    }
    pub fn play(&mut self, from: f64) {
        if !self.has_buffer() {
            return;
        }
        self.seek(from);
        if self.sh.dead.swap(false, Ordering::Relaxed) {
            // device lost (unplugged / default output changed): rebuild the stream
            self.stream = None;
        }
        if self.stream.is_none() {
            match self.build() {
                Ok(s) => self.stream = Some(s),
                Err(e) => {
                    self.error = format!("uscita audio non disponibile: {e}");
                    return;
                }
            }
        }
        if let Some(s) = &self.stream {
            if let Err(e) = s.play() {
                // try once more with a fresh stream
                self.stream = None;
                match self.build() {
                    Ok(ns) => {
                        let _ = ns.play();
                        self.stream = Some(ns);
                    }
                    Err(e2) => {
                        self.error = format!("impossibile avviare l'audio: {e} / {e2}");
                        return;
                    }
                }
            }
        }
        self.sh.playing.store(true, Ordering::Relaxed);
    }
    pub fn set_metronome(&self, beats: Option<Arc<Vec<(f64, bool)>>>) {
        if let Ok(mut g) = self.sh.metro.lock() {
            *g = beats;
        }
    }
    pub fn set_metronome_on(&self, on: bool) {
        self.sh.metro_on.store(on, Ordering::Relaxed);
    }
    pub fn metronome_on(&self) -> bool {
        self.sh.metro_on.load(Ordering::Relaxed)
    }
    pub fn stop(&mut self) {
        self.sh.playing.store(false, Ordering::Relaxed);
    }

    fn build(&self) -> Result<cpal::Stream, String> {
        let host = cpal::default_host();
        let dev = host.default_output_device().ok_or("Nessuna uscita audio trovata")?;
        let cfg = dev.default_output_config().map_err(|e| e.to_string())?;
        let fmt = cfg.sample_format();
        let config: cpal::StreamConfig = cfg.into();
        match fmt {
            cpal::SampleFormat::F32 => self.build_t::<f32>(&dev, &config),
            cpal::SampleFormat::I16 => self.build_t::<i16>(&dev, &config),
            cpal::SampleFormat::U16 => self.build_t::<u16>(&dev, &config),
            cpal::SampleFormat::I32 => self.build_t::<i32>(&dev, &config),
            other => Err(format!("Formato audio non supportato: {other:?}")),
        }
    }

    fn build_t<T>(&self, dev: &cpal::Device, config: &cpal::StreamConfig) -> Result<cpal::Stream, String>
    where
        T: cpal::SizedSample + cpal::FromSample<f32>,
    {
        let sh = self.sh.clone();
        let sh_err = self.sh.clone();
        let channels = config.channels as usize;
        let dev_sr = config.sample_rate.0 as f64;
        let stream = dev
            .build_output_stream(
                config,
                move |data: &mut [T], _| {
                    let playing = sh.playing.load(Ordering::Relaxed);
                    let vol = f32::from_bits(sh.volume.load(Ordering::Relaxed));
                    let guard = sh.buf.try_lock();
                    let buf = match (&guard, playing) {
                        (Ok(g), true) => g.as_ref().cloned(),
                        _ => None,
                    };
                    drop(guard);
                    match buf {
                        Some(b) => {
                            let step = b.sr as f64 / dev_sr;
                            let mut pos = f64::from_bits(sh.pos.load(Ordering::Relaxed));
                            let metro = if sh.metro_on.load(Ordering::Relaxed) {
                                sh.metro.try_lock().ok().and_then(|g| g.clone())
                            } else {
                                None
                            };
                            let bsr = b.sr.max(1) as f64;
                            for frame in data.chunks_mut(channels.max(1)) {
                                let i = pos as usize;
                                let (mut l, mut r) = if i * 2 + 1 < b.data.len() { (b.data[i * 2], b.data[i * 2 + 1]) } else { (0.0, 0.0) };
                                if let Some(m) = &metro {
                                    let t = pos / bsr;
                                    let k = m.partition_point(|x| x.0 <= t);
                                    if k > 0 {
                                        let (ct, down) = m[k - 1];
                                        let dt = t - ct;
                                        if dt < 0.035 {
                                            let f = if down { 1700.0 } else { 1100.0 };
                                            let c = ((dt * f * std::f64::consts::TAU).sin() * (-dt / 0.01).exp() * 0.55) as f32;
                                            l += c;
                                            r += c;
                                        }
                                    }
                                }
                                for (c, s) in frame.iter_mut().enumerate() {
                                    let v = if c == 0 { l } else if c == 1 { r } else { 0.0 };
                                    *s = T::from_sample(v * vol);
                                }
                                pos += step;
                            }
                            sh.pos.store(pos.to_bits(), Ordering::Relaxed);
                        }
                        None => {
                            for s in data.iter_mut() {
                                *s = T::from_sample(0.0f32);
                            }
                        }
                    }
                },
                move |e| {
                    eprintln!("audio: {e}");
                    sh_err.dead.store(true, Ordering::Relaxed);
                },
                None,
            )
            .map_err(|e| e.to_string())?;
        Ok(stream)
    }
}
