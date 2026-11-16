//! State shared between the window, the MIDI listener and the DMX thread.

use crate::model::*;
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::Instant;

/// MIDI note numbers with a fixed meaning.
pub const NOTE_PLAY_FROM_START: u8 = 120;
pub const NOTE_STOP: u8 = 121;
pub const NOTE_BLACKOUT: u8 = 122;
pub const MAX_CUE_NOTE: u8 = 119;

#[derive(Clone, Debug)]
pub struct Transport {
    pub song: usize,
    pub playing: bool,
    pub anchor: Instant,
    pub anchor_pos: f64,
    /// Incremented on every change, so the window can react.
    pub gen: u64,
    pub by_midi: bool,
}

impl Default for Transport {
    fn default() -> Self {
        Transport { song: 0, playing: false, anchor: Instant::now(), anchor_pos: 0.0, gen: 0, by_midi: false }
    }
}

impl Transport {
    pub fn pos(&self) -> f64 {
        if self.playing {
            self.anchor_pos + self.anchor.elapsed().as_secs_f64()
        } else {
            self.anchor_pos
        }
    }
    pub fn play_from(&mut self, pos: f64, at: Instant, by_midi: bool) {
        self.playing = true;
        self.anchor = at;
        self.anchor_pos = pos.max(0.0);
        self.by_midi = by_midi;
        self.gen += 1;
    }
    pub fn pause(&mut self) {
        self.anchor_pos = self.pos();
        self.playing = false;
        self.gen += 1;
    }
    pub fn stop(&mut self, by_midi: bool) {
        self.playing = false;
        self.anchor_pos = 0.0;
        self.by_midi = by_midi;
        self.gen += 1;
    }
    pub fn seek(&mut self, pos: f64) {
        self.anchor_pos = pos.max(0.0);
        self.anchor = Instant::now();
        self.gen += 1;
    }
    /// Re-align to an external clock (audio player) without counting as a change.
    pub fn realign(&mut self, pos: f64) {
        if self.playing {
            self.anchor = Instant::now();
            self.anchor_pos = pos;
        }
    }
}

pub struct DmxConfig {
    pub port: Option<String>,
    pub protocol: DmxProtocol,
    pub fps: u32,
}

pub struct Shared {
    pub project: Arc<Project>,
    pub transport: Transport,
    pub blackout: bool,
    pub identify: Option<(Vec<u32>, Instant)>,
    pub universe: [u8; 512],
    pub dmx: DmxConfig,
    pub dmx_status: String,
    pub dmx_frames: u64,
    pub midi_log: VecDeque<String>,
    pub midi_clock_count: u64,
    pub last_midi: Option<Instant>,
    /// Manual channel overrides from the test panel (channel index, value).
    pub test_channels: Option<Vec<(usize, u8)>>,
    /// Master intensity received by MIDI CC7, picked up by the window.
    pub pending_master: Option<f32>,
}

pub type SharedHandle = Arc<Mutex<Shared>>;

impl Shared {
    pub fn new(project: Project) -> SharedHandle {
        let protocol = project.settings.dmx_protocol;
        let fps = project.settings.dmx_fps;
        Arc::new(Mutex::new(Shared {
            project: Arc::new(project),
            transport: Transport::default(),
            blackout: false,
            identify: None,
            universe: [0; 512],
            dmx: DmxConfig { port: None, protocol, fps },
            dmx_status: "Uscita DMX non collegata".into(),
            dmx_frames: 0,
            midi_log: VecDeque::new(),
            midi_clock_count: 0,
            last_midi: None,
            test_channels: None,
            pending_master: None,
        }))
    }
    pub fn log(&mut self, s: String) {
        let ts = chrono_like();
        self.midi_log.push_front(format!("{ts}  {s}"));
        while self.midi_log.len() > 200 {
            self.midi_log.pop_back();
        }
    }

    /// React to one incoming MIDI message.
    pub fn handle_midi(&mut self, msg: &[u8], at: Instant) {
        if msg.is_empty() {
            return;
        }
        let status = msg[0];
        self.last_midi = Some(at);
        match status {
            0xF8 => {
                self.midi_clock_count += 1;
                return;
            }
            0xFA => {
                self.transport.play_from(0.0, at, true);
                self.log("MIDI Start → show dall'inizio".into());
                return;
            }
            0xFB => {
                let p = self.transport.anchor_pos;
                self.transport.play_from(p, at, true);
                self.log("MIDI Continue → riprende".into());
                return;
            }
            0xFC => {
                self.transport.pause();
                self.transport.by_midi = true;
                self.log("MIDI Stop → pausa".into());
                return;
            }
            0xF2 if msg.len() >= 3 => {
                let sixteenths = msg[1] as u32 | ((msg[2] as u32) << 7);
                let song = self.project.songs.get(self.transport.song);
                let spb = song.and_then(|s| s.analysis.as_ref()).map(|a| a.spb()).unwrap_or(0.5);
                let pos = sixteenths as f64 * spb / 4.0;
                self.transport.seek(pos);
                self.log(format!("Song Position → {:.2}s", pos));
                return;
            }
            0xF0..=0xFF => return,
            _ => {}
        }
        let kind = status & 0xF0;
        let chan = (status & 0x0F) + 1;
        let want = self.project.settings.midi_channel;
        if want != 0 && chan != want {
            self.log(format!("Ignorato (canale {chan}, ascolto il {want})"));
            return;
        }
        match kind {
            0xC0 if msg.len() >= 2 => {
                let prog = msg[1];
                if let Some(i) = self.project.songs.iter().position(|s| s.program == prog) {
                    self.transport.song = i;
                    self.transport.stop(true);
                    let name = self.project.songs[i].name.clone();
                    self.log(format!("Program Change {prog} → brano «{name}»"));
                } else {
                    self.log(format!("Program Change {prog}: nessun brano con questo numero"));
                }
            }
            0x90 if msg.len() >= 3 && msg[2] > 0 => {
                let note = msg[1];
                let si = self.transport.song;
                let Some(song) = self.project.songs.get(si) else {
                    self.log(format!("Nota {note}: nessun brano selezionato"));
                    return;
                };
                match note {
                    NOTE_PLAY_FROM_START => {
                        self.blackout = false;
                        self.transport.play_from(0.0, at, true);
                        self.log(format!("Nota {note} → «{}» dall'inizio", song.name));
                    }
                    NOTE_STOP => {
                        self.transport.stop(true);
                        self.log(format!("Nota {note} → stop"));
                    }
                    NOTE_BLACKOUT => {
                        self.blackout = !self.blackout;
                        self.log(format!("Nota {note} → blackout {}", if self.blackout { "ON" } else { "OFF" }));
                    }
                    n if n <= MAX_CUE_NOTE => {
                        if let Some(c) = song.cues.get(n as usize) {
                            let (t, name) = (c.time, c.name.clone());
                            self.blackout = false;
                            self.transport.play_from(t, at, true);
                            self.log(format!("Nota {n} → scena «{name}» ({:.2}s)", t));
                        } else {
                            self.log(format!("Nota {n}: il brano non ha una scena {}", n));
                        }
                    }
                    _ => self.log(format!("Nota {note}: non usata")),
                }
            }
            0x90 | 0x80 => {}
            0xB0 if msg.len() >= 3 => {
                // CC 7 = master intensity (handy from a pedal / controller)
                if msg[1] == 7 {
                    let m = msg[2] as f32 / 127.0;
                    let mut p = (*self.project).clone();
                    p.settings.master = m;
                    self.project = Arc::new(p);
                    self.pending_master = Some(m);
                    self.log(format!("CC7 → master {}%", (msg[2] as u32 * 100) / 127));
                }
            }
            _ => self.log(format!("Messaggio MIDI {:02X?}", msg)),
        }
    }
}

fn chrono_like() -> String {
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
    let s = (now / 1000) % 86400;
    format!("{:02}:{:02}:{:02}.{:03}", s / 3600, (s / 60) % 60, s % 60, now % 1000)
}
