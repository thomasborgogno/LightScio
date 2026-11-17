//! MIDI: writing the control track (.mid) and listening to MIDI input live.

use crate::live::*;
use crate::model::*;
use std::time::Instant;

const PPQ: u16 = 480;

fn vlq(mut v: u32, out: &mut Vec<u8>) {
    let mut buf = vec![(v & 0x7F) as u8];
    v >>= 7;
    while v > 0 {
        buf.push(((v & 0x7F) as u8) | 0x80);
        v >>= 7;
    }
    buf.reverse();
    out.extend(buf);
}

fn track_chunk(events: &mut [(u32, Vec<u8>)]) -> Vec<u8> {
    events.sort_by_key(|e| e.0);
    let mut data = vec![];
    let mut last = 0u32;
    for (t, ev) in events.iter() {
        vlq(t - last, &mut data);
        data.extend(ev);
        last = *t;
    }
    vlq(0, &mut data);
    data.extend([0xFF, 0x2F, 0x00]);
    let mut out = b"MTrk".to_vec();
    out.extend((data.len() as u32).to_be_bytes());
    out.extend(data);
    out
}

pub struct ExportEvent {
    pub time: f64,
    pub what: String,
}

/// Build a standard MIDI file (type 1) driving the show of one song.
/// Returns the file bytes and a human readable list of the events.
pub fn export_song(song: &Song, settings: &Settings) -> (Vec<u8>, Vec<ExportEvent>) {
    let bpm = song.analysis.as_ref().map(|a| a.bpm).unwrap_or(120.0).max(20.0);
    let bpb = song.analysis.as_ref().map(|a| a.beats_per_bar).unwrap_or(4);
    let ticks = |t: f64| (t.max(0.0) * bpm / 60.0 * PPQ as f64).round() as u32;
    let ch = settings.midi_channel.clamp(1, 16) - 1;
    let note_len = (PPQ / 4) as u32;
    let mut list = vec![];

    // tempo track
    let us_per_q = (60_000_000.0 / bpm).round() as u32;
    let mut t0: Vec<(u32, Vec<u8>)> = vec![];
    let mut name = vec![0xFF, 0x03];
    let nb = song.name.as_bytes();
    vlq(nb.len() as u32, &mut name);
    name.extend(nb);
    t0.push((0, name));
    t0.push((0, vec![0xFF, 0x51, 0x03, (us_per_q >> 16) as u8, (us_per_q >> 8) as u8, us_per_q as u8]));
    t0.push((0, vec![0xFF, 0x58, 0x04, bpb as u8, 2, 24, 8]));

    // control track
    let mut t1: Vec<(u32, Vec<u8>)> = vec![];
    let tn = b"Luci";
    let mut tname = vec![0xFF, 0x03];
    vlq(tn.len() as u32, &mut tname);
    tname.extend(tn);
    t1.push((0, tname));
    t1.push((0, vec![0xC0 | ch, song.program & 0x7F]));
    list.push(ExportEvent { time: 0.0, what: format!("Program Change {} (seleziona il brano)", song.program) });
    let note = |t1: &mut Vec<(u32, Vec<u8>)>, at: u32, n: u8| {
        // order: note-on slightly after the program change at tick 0
        t1.push((at, vec![0x90 | ch, n, 100]));
        t1.push((at + note_len, vec![0x80 | ch, n, 0]));
    };
    let first_cue_at_zero = song.cues.first().map(|c| c.time < 0.01).unwrap_or(false);
    if !first_cue_at_zero {
        note(&mut t1, 0, NOTE_PLAY_FROM_START);
        list.push(ExportEvent { time: 0.0, what: format!("Nota {NOTE_PLAY_FROM_START} (avvia lo show)") });
    }
    for (i, c) in song.cues.iter().enumerate().take(MAX_CUE_NOTE as usize + 1) {
        note(&mut t1, ticks(c.time), i as u8);
        list.push(ExportEvent { time: c.time, what: format!("Nota {i} → scena «{}»", c.name) });
    }
    if settings.stop_note_at_end {
        let end = song.duration();
        note(&mut t1, ticks(end), NOTE_STOP);
        list.push(ExportEvent { time: end, what: format!("Nota {NOTE_STOP} (fine show)") });
    }

    let mut out = b"MThd".to_vec();
    out.extend(6u32.to_be_bytes());
    out.extend(1u16.to_be_bytes());
    out.extend(2u16.to_be_bytes());
    out.extend(PPQ.to_be_bytes());
    out.extend(track_chunk(&mut t0));
    out.extend(track_chunk(&mut t1));
    (out, list)
}

pub fn note_name(n: u8) -> String {
    const NAMES: [&str; 12] = ["Do", "Do#", "Re", "Re#", "Mi", "Fa", "Fa#", "Sol", "Sol#", "La", "La#", "Si"];
    const NAMES_EN: [&str; 12] = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];
    let names = if crate::i18n::is_english() { NAMES_EN } else { NAMES };
    format!("{}{}", names[(n % 12) as usize], n as i32 / 12 - 1)
}

// ---------------------------------------------------------------- input

pub struct MidiIn {
    pub conn: Option<midir::MidiInputConnection<()>>,
    pub port_name: String,
    pub error: String,
}

impl MidiIn {
    pub fn new() -> Self {
        MidiIn { conn: None, port_name: String::new(), error: String::new() }
    }
    pub fn ports() -> Vec<String> {
        match midir::MidiInput::new("LightScio scan") {
            Ok(m) => m.ports().iter().filter_map(|p| m.port_name(p).ok()).collect(),
            Err(_) => vec![],
        }
    }
    pub fn connect(&mut self, name: &str, shared: SharedHandle) {
        self.disconnect();
        let mut input = match midir::MidiInput::new("LightScio") {
            Ok(m) => m,
            Err(e) => {
                self.error = format!("MIDI non disponibile: {e}");
                return;
            }
        };
        input.ignore(midir::Ignore::None);
        let ports = input.ports();
        let Some(port) = ports.iter().find(|p| input.port_name(p).ok().as_deref() == Some(name)) else {
            self.error = format!("Porta «{name}» non trovata");
            return;
        };
        let sh = shared.clone();
        match input.connect(
            port,
            "lightscio-in",
            move |_stamp, msg, _| {
                let at = Instant::now();
                if let Ok(mut s) = sh.lock() {
                    s.handle_midi(msg, at);
                }
            },
            (),
        ) {
            Ok(c) => {
                self.conn = Some(c);
                self.port_name = name.to_string();
                self.error.clear();
                if let Ok(mut s) = shared.lock() {
                    s.log(format!("In ascolto su «{name}»"));
                }
            }
            Err(e) => self.error = format!("Impossibile aprire «{name}»: {e}"),
        }
    }
    pub fn disconnect(&mut self) {
        if let Some(c) = self.conn.take() {
            c.close();
        }
        self.port_name.clear();
    }
}
