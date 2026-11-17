//! Automatic light-show programming from the analysis, with built-in rules
//! or by asking the locally installed Claude Code (`claude -p`).

use crate::model::*;
use serde_json::{json, Value};

const PALETTES: &[(&str, [[u8; 3]; 3])] = &[
    ("Ghiaccio", [[30, 80, 255], [0, 200, 255], [160, 220, 255]]),
    ("Fuoco", [[255, 40, 0], [255, 140, 0], [255, 210, 80]]),
    ("Magenta", [[255, 0, 140], [140, 0, 255], [255, 90, 200]]),
    ("Oceano", [[0, 110, 255], [0, 255, 170], [0, 60, 160]]),
    ("Tramonto", [[255, 80, 30], [255, 0, 110], [255, 170, 60]]),
    ("Bosco", [[40, 255, 80], [0, 170, 120], [190, 255, 60]]),
    ("Viola", [[120, 0, 255], [210, 0, 255], [60, 40, 255]]),
    ("Rosso", [[255, 0, 0], [200, 0, 40], [255, 70, 40]]),
];

const WARM_WHITE: [u8; 3] = [255, 214, 170];

fn seed(s: &str) -> usize {
    s.bytes().fold(7usize, |a, b| a.wrapping_mul(31).wrapping_add(b as usize))
}

fn lighten(c: [u8; 3], k: f32) -> [u8; 3] {
    [
        (c[0] as f32 + (255.0 - c[0] as f32) * k) as u8,
        (c[1] as f32 + (255.0 - c[1] as f32) * k) as u8,
        (c[2] as f32 + (255.0 - c[2] as f32) * k) as u8,
    ]
}

pub struct Pals {
    cool: [[u8; 3]; 3],
    verse: [[u8; 3]; 3],
    chorus: [[u8; 3]; 3],
    other: [[u8; 3]; 3],
}

fn palettes(song: &Song) -> Pals {
    let ch = song.analysis.as_ref().map(|a| a.character.clone()).unwrap_or_default();
    let s = seed(&song.name);
    let pick = |opts: &[usize], k: usize| PALETTES[opts[(s + k) % opts.len()]].1;
    let (verse, chorus, other) = if ch.starts_with("Rock") {
        (pick(&[0, 3, 6], 0), pick(&[1, 7, 4], 1), pick(&[2, 6], 2))
    } else if ch.starts_with("Ballad") {
        (pick(&[0, 3], 0), pick(&[6, 2, 4], 1), pick(&[5, 3], 2))
    } else if ch.starts_with("Dance") {
        (pick(&[3, 6], 0), pick(&[2, 4, 5], 1), pick(&[0, 1], 2))
    } else {
        (pick(&[0, 3, 5, 6], 0), pick(&[1, 2, 4, 7], 1), pick(&[6, 5, 2], 2))
    };
    Pals { cool: pick(&[0, 6, 3], 3), verse, chorus, other }
}

fn look(intensity: f32, colors: Vec<[u8; 3]>, effect: Effect, rate: f32) -> GroupLook {
    GroupLook { intensity, colors, effect, rate, ..GroupLook::default() }
}

fn moving(intensity: f32, colors: Vec<[u8; 3]>, mv: Movement, size: f32, rate: f32, tilt: f32) -> GroupLook {
    GroupLook { intensity, colors, movement: mv, move_size: size, move_rate: rate, tilt, ..GroupLook::default() }
}

/// The look of every group for a section kind / intensity. `var` gives variations.
pub fn section_looks(kind: SectionKind, i: f32, pals: &Pals, var: usize, dance: bool) -> Vec<GroupLook> {
    let pal = match kind {
        SectionKind::Intro | SectionKind::Outro => pals.cool,
        SectionKind::Verse => pals.verse,
        SectionKind::PreChorus => [pals.verse[0], pals.chorus[0], pals.verse[1]],
        SectionKind::Chorus => pals.chorus,
        SectionKind::Bridge | SectionKind::Solo | SectionKind::Other => pals.other,
        SectionKind::Break => [pals.verse[0], pals.verse[0], pals.verse[0]],
    };
    let rot = |k: usize| -> Vec<[u8; 3]> {
        let mut v = pal.to_vec();
        v.rotate_left((k + var) % 3);
        v
    };
    let mut looks = vec![GroupLook::default(); Group::ALL.len()];

    // front wash: lights the band, mostly warm white
    let front_col = if matches!(kind, SectionKind::Chorus | SectionKind::Solo) && i > 0.6 {
        vec![lighten(pal[0], 0.55)]
    } else {
        vec![WARM_WHITE]
    };
    // front wash lights the band: always steady
    let fw = look(0.3 + 0.5 * i, front_col, Effect::Static, 1.0);
    looks[Group::FrontWash.idx()] = fw;

    // backlight PARs: the main color. Steady by default (a rock show must not
    // look like a disco); only dance songs follow the kick drum.
    let (eff, rate) = if kind == SectionKind::Break || i < 0.25 {
        (Effect::Breath, 8.0)
    } else if dance && i >= 0.65 {
        if var % 2 == 0 {
            (Effect::Kick, 1.0)
        } else {
            (Effect::Chase, 0.5)
        }
    } else if dance && i >= 0.45 {
        (Effect::Pulse, 2.0)
    } else {
        (Effect::Static, 1.0)
    };
    let mut bw = look(0.45 + 0.55 * i, rot(0), eff, rate);
    bw.spread = 0.6;
    if kind == SectionKind::Break {
        bw.intensity = 0.3;
    }
    looks[Group::BackWash.idx()] = bw;

    // moving heads
    let (mv, size, mrate, tilt) = if kind == SectionKind::Break {
        (Movement::Still, 0.0, 8.0, 8.0)
    } else if i < 0.25 {
        (Movement::Circle, 0.25, 16.0, 30.0)
    } else if i < 0.45 {
        (Movement::Figure8, 0.35, 8.0, 32.0)
    } else if i < 0.65 {
        (if var % 2 == 0 { Movement::PanSweep } else { Movement::Fan }, 0.5, 4.0, 35.0)
    } else if i < 0.8 {
        (Movement::Circle, 0.7, 2.0, 38.0)
    } else if var % 2 == 0 {
        (Movement::Ballyhoo, 0.8, 2.0, 40.0)
    } else {
        (Movement::Crowd, 0.6, 2.0, 70.0)
    };
    let head_int = if kind == SectionKind::Break { 0.35 } else { 0.55 + 0.45 * i };
    let mut hf = moving(head_int, rot(1), mv, size, mrate, tilt);
    hf.spread = 0.5;
    let mut hb = moving(head_int, rot(2), mv, size, mrate, tilt + 5.0);
    hb.spread = 0.25;
    // symmetric look (left side mirrors the right) on most moving looks
    let mirror = matches!(mv, Movement::Circle | Movement::Figure8 | Movement::PanSweep | Movement::Ballyhoo) && var % 3 != 2;
    for h in [&mut hf, &mut hb] {
        h.mirror = mirror;
        if mirror {
            h.spread = 0.0;
        }
    }
    if kind == SectionKind::Chorus && i > 0.7 {
        hb.colors = vec![[255, 255, 255], pal[0]];
    }
    looks[Group::HeadsFront.idx()] = hf;
    looks[Group::HeadsBack.idx()] = hb;

    // blinders & strobes are used through hits
    looks[Group::Blinders.idx()] = look(0.0, vec![[255, 210, 140]], Effect::Static, 1.0);
    looks[Group::Strobes.idx()] = look(0.0, vec![[255, 255, 255]], Effect::Static, 1.0);

    // LED bars
    let bars = if dance && i > 0.5 {
        look(0.4 + 0.6 * i, rot(1), Effect::Chase, 0.5)
    } else if i > 0.5 {
        look(0.4 + 0.6 * i, rot(1), Effect::Static, 1.0)
    } else {
        look(0.3 + 0.5 * i, rot(1), Effect::Breath, 8.0)
    };
    looks[Group::Bars.idx()] = bars;
    looks[Group::Extra.idx()] = look(0.4 + 0.5 * i, rot(0), Effect::Static, 1.0);
    looks
}

/// Build the whole show (cues + hits) from the analysis with built-in rules.
pub fn generate(song: &Song) -> (Vec<Cue>, Vec<Hit>) {
    let Some(a) = song.analysis.as_ref() else { return (vec![], vec![]) };
    let pals = palettes(song);
    let dance = a.character.starts_with("Dance");
    let bar = a.bar_len();
    let mut cues = vec![];
    let mut hits = vec![];
    for (si, s) in song.sections.iter().enumerate() {
        let prev_i = if si > 0 { song.sections[si - 1].intensity } else { 0.0 };
        let calm = s.intensity < 0.35 || matches!(s.kind, SectionKind::Intro | SectionKind::Outro);
        let fade = if si == 0 {
            1.5
        } else if calm && s.intensity <= prev_i {
            1.2
        } else {
            0.05
        };
        let mut c = Cue { time: s.start, name: s.label.clone(), fade, looks: section_looks(s.kind, s.intensity, &pals, 0, dance) };
        c.fix();
        cues.push(c);
        // variation every 8 bars inside long sections
        let bars = ((s.end - s.start) / bar).round() as usize;
        if bars >= 16 {
            let mut k = 8;
            let mut var = 1;
            while k + 4 <= bars {
                let t = s.start + k as f64 * bar;
                let mut c = Cue {
                    time: t,
                    name: format!("{} · var {}", s.label, var),
                    fade: 0.05,
                    looks: section_looks(s.kind, s.intensity, &pals, var, dance),
                };
                c.fix();
                cues.push(c);
                k += 8;
                var += 1;
            }
        }
        // passage into the next part: build-up before a strong chorus, or a
        // drum roll (dense snare hits in the last bar). Only here the PARs
        // follow the drums.
        let next = song.sections.get(si + 1);
        if let Some(n) = next {
            let bpb = a.beats_per_bar.max(1) as usize;
            let in_range = |t0: f64, t1: f64| a.snares.iter().filter(|&&x| x >= t0 && x < t1).count();
            let roll = s.kind != SectionKind::Break
                && (s.end - s.start) >= bar * 2.0
                && (in_range(s.end - bar, s.end) >= bpb + 3 || in_range(s.end - bar / 2.0, s.end) >= 4);
            let build = n.kind == SectionKind::Chorus && n.intensity > 0.6 && s.kind != SectionKind::Break && (s.end - s.start) >= bar * 2.0;
            if build || roll {
                let t = s.end - bar;
                let mut looks = section_looks(s.kind, (s.intensity + 0.2).min(1.0), &pals, 1, dance);
                if roll {
                    for g in [Group::BackWash, Group::Bars] {
                        let l = &mut looks[g.idx()];
                        l.effect = Effect::Snare;
                        l.colors = vec![[255, 255, 255], pals.chorus[0]];
                    }
                }
                if build {
                    looks[Group::HeadsFront.idx()].movement = Movement::TiltSweep;
                    looks[Group::HeadsFront.idx()].move_rate = 2.0;
                    looks[Group::HeadsBack.idx()].movement = Movement::TiltSweep;
                    looks[Group::HeadsBack.idx()].move_rate = 2.0;
                }
                let name = if roll { crate::i18n::t(&format!("Rullata verso {}", n.label)) } else { crate::i18n::t(&format!("Salita verso {}", n.label)) };
                let mut c = Cue { time: t, name, fade: 0.05, looks };
                c.fix();
                cues.push(c);
                if build && n.intensity > 0.85 {
                    hits.push(Hit { time: n.start - a.spb() * 2.0, kind: HitKind::Strobe, duration: a.spb() * 2.0 - 0.02 });
                }
            }
            // blinder punch on strong section starts
            if n.intensity > 0.6 && n.intensity > s.intensity + 0.1 {
                hits.push(Hit { time: n.start, kind: HitKind::Blinder, duration: 0.15 });
            }
        }
    }
    // stops and accents
    for &(t, d) in &a.stops {
        hits.push(Hit { time: t, kind: HitKind::Blackout, duration: (d - 0.02).max(0.05) });
    }
    for &(t, strength) in &a.accents {
        if hits.iter().any(|h| h.kind == HitKind::Blinder && (h.time - t).abs() < 0.3) {
            continue;
        }
        let kind = if strength > 0.75 { HitKind::Blinder } else { HitKind::Flash };
        hits.push(Hit { time: t, kind, duration: if kind == HitKind::Blinder { 0.2 } else { 0.08 } });
    }
    // fade to black at the end
    let mut end = Cue::blank((a.duration - 0.2).max(0.0), crate::i18n::ts("Buio finale"));
    end.fade = 2.5;
    if cues.last().map(|c| c.time < end.time - 1.0).unwrap_or(true) {
        cues.push(end);
    }
    cues.sort_by(|x, y| x.time.total_cmp(&y.time));
    hits.sort_by(|x, y| x.time.total_cmp(&y.time));
    (cues, hits)
}

// ---------------------------------------------------------------- Claude Code

fn effect_names() -> Vec<String> {
    Effect::ALL.iter().map(|e| format!("{e:?}")).collect()
}
fn movement_names() -> Vec<String> {
    Movement::ALL.iter().map(|e| format!("{e:?}")).collect()
}

pub fn claude_prompt(song: &Song, project: &Project, catalog: &[FixtureDef]) -> String {
    let a = song.analysis.clone().unwrap_or_default();
    let sections: Vec<Value> = song
        .sections
        .iter()
        .map(|s| {
            json!({
                "label": s.label, "kind": format!("{:?}", s.kind),
                "start_s": (s.start * 1000.0).round() / 1000.0, "end_s": (s.end * 1000.0).round() / 1000.0,
                "start_bar": a.bar_beat(s.start), "intensity_0_1": (s.intensity * 100.0).round() / 100.0
            })
        })
        .collect();
    let mut groups = vec![];
    for g in Group::ALL {
        let members: Vec<&FixtureInstance> = project.fixtures.iter().filter(|f| f.group == g).collect();
        if members.is_empty() {
            continue;
        }
        let types: Vec<String> = members
            .iter()
            .filter_map(|f| project.def(catalog, &f.def_id))
            .map(|d| format!("{} ({})", d.kind.label(), d.capabilities()))
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect();
        groups.push(json!({"group": format!("{g:?}"), "description": g.label(), "count": members.len(), "fixture_types": types}));
    }
    let info = json!({
        "song": song.name, "bpm": a.bpm, "beats_per_bar": a.beats_per_bar, "first_downbeat_s": a.offset,
        "duration_s": a.duration, "character": a.character, "sections": sections,
        "accents_s": a.accents.iter().map(|(t, s)| json!([((t*1000.0).round()/1000.0), ((s*100.0).round()/100.0)])).collect::<Vec<_>>(),
        "stops_s": a.stops.iter().map(|(t, d)| json!({"start": (t*1000.0).round()/1000.0, "duration": (d*1000.0).round()/1000.0})).collect::<Vec<_>>(),
        "rig_groups": groups,
    });
    format!(
        r#"You are an expert concert lighting designer programming a live rock/pop band show.
Design the light show for this song, using the audio analysis and the lighting rig below.

ANALYSIS AND RIG (JSON):
{info}

Reply with ONLY one JSON object (no prose, no code fences) with this exact shape:
{{"cues":[{{"time":0.0,"name":"Intro","fade":1.0,"looks":{{"GROUP":LOOK}}}}],"hits":[{{"time":12.34,"kind":"Blinder","duration":0.2}}]}}

- "time" in seconds from the song start; place cues on beats (beat length = 60/bpm s, first downbeat at first_downbeat_s).
- Use at least one cue per section and change looks every 8 bars inside long sections; between 10 and 60 cues.
- "name": short {lang} name (e.g. {ex}).
- "fade": seconds of crossfade into the cue (0.05 for punchy changes, 1-3 for calm ones).
- GROUP keys: only those listed in rig_groups ({gk}).
- LOOK = {{"intensity":0..1,"colors":[[r,g,b],...1-3 colors 0-255],"effect":E,"rate":beats per effect cycle (0.25-16),"movement":M,"move_size":0..1,"move_rate":beats per movement cycle (1-32),"tilt":degrees from vertical towards the audience (-30..80),"spread":0..1 phase spread between fixtures,"strobe":0..1 (0 = off),"mirror":true/false (moving heads on the left side do the movement mirrored, for symmetric looks)}}
- E is one of {eff}; M is one of {mov} (movement only matters for moving heads).
- Blinders and Strobes should normally stay at intensity 0 in looks and fire through "hits".
- "hits" kinds: "Blinder" (audience blinders punch), "Strobe" (strobe burst for "duration" seconds), "Flash" (all fixtures white flash), "Blackout" (everything dark for "duration" — use for stops).
- Put a Blackout on every stop, a Blinder on the hit right after a stop and on the strongest accents and chorus downbeats. Do not overuse strobe.
- Match energy: calm sections = slow breath/circles, saturated deep colors, low intensity; big choruses = brighter colors, bigger and faster moving-head movements, white beams.
- IMPORTANT: PARs, front wash and LED bars must normally stay steady ("Static", or "Breath" in calm parts). Do NOT make them follow the kick/snare or pulse on the beat: for a rock/pop band that looks like a disco. Use "Snare", "Pulse" or "Chase" only rarely: on a real drum roll or a short transition (last bar before a new part), or for clearly dance/electronic songs.
- End with a cue that fades everything to black at the end of the song.
"#,
        info = serde_json::to_string_pretty(&info).unwrap_or_default(),
        gk = Group::ALL.iter().map(|g| format!("{g:?}")).collect::<Vec<_>>().join(", "),
        eff = effect_names().join(", "),
        mov = movement_names().join(", "),
        lang = if crate::i18n::is_english() { "English" } else { "Italian" },
        ex = if crate::i18n::is_english() { "\"Verse 1\", \"Chorus 2 - variation\"" } else { "\"Strofa 1\", \"Ritornello 2 - variazione\"" },
    )
}

/// Run `claude -p` with the prompt on stdin. Blocking: call from a thread.
pub fn run_claude(cmd: &str, prompt: &str) -> Result<String, String> {
    use std::io::Write;
    use std::process::{Command, Stdio};
    let cmd = if cmd.trim().is_empty() { "claude" } else { cmd.trim() };
    let mut candidates = vec![cmd.to_string()];
    if cfg!(windows) && !cmd.contains('.') {
        candidates.push(format!("{cmd}.cmd"));
        candidates.push(format!("{cmd}.exe"));
    }
    let mut last_err = String::new();
    for c in candidates {
        let mut command = Command::new(&c);
        command
            .arg("-p")
            .arg("--output-format")
            .arg("text")
            .current_dir(std::env::temp_dir())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x0800_0000);
        }
        match command.spawn() {
            Ok(mut child) => {
                if let Some(mut stdin) = child.stdin.take() {
                    let _ = stdin.write_all(prompt.as_bytes());
                }
                let out = child.wait_with_output().map_err(|e| e.to_string())?;
                let text = String::from_utf8_lossy(&out.stdout).to_string();
                if !out.status.success() && text.trim().is_empty() {
                    return Err(format!("Claude Code ha restituito un errore: {}", String::from_utf8_lossy(&out.stderr)));
                }
                return Ok(text);
            }
            Err(e) => last_err = format!("{c}: {e}"),
        }
    }
    Err(format!("Non trovo Claude Code sul computer ({last_err}). Controlla il comando nelle Impostazioni."))
}

fn parse_effect(v: &Value) -> Option<Effect> {
    let s = v.as_str()?.to_lowercase();
    Effect::ALL.iter().copied().find(|e| format!("{e:?}").to_lowercase() == s)
}
fn parse_movement(v: &Value) -> Option<Movement> {
    let s = v.as_str()?.to_lowercase();
    Movement::ALL.iter().copied().find(|e| format!("{e:?}").to_lowercase() == s)
}

fn parse_look(v: &Value) -> GroupLook {
    let mut l = GroupLook::default();
    let f = |k: &str| v.get(k).and_then(|x| x.as_f64()).map(|x| x as f32);
    if let Some(x) = f("intensity") {
        l.intensity = x.clamp(0.0, 1.0);
    }
    if let Some(arr) = v.get("colors").and_then(|c| c.as_array()) {
        let cols: Vec<[u8; 3]> = arr
            .iter()
            .filter_map(|c| {
                let a = c.as_array()?;
                let g = |i: usize| a.get(i).and_then(|x| x.as_f64()).unwrap_or(0.0).clamp(0.0, 255.0) as u8;
                Some([g(0), g(1), g(2)])
            })
            .collect();
        if !cols.is_empty() {
            l.colors = cols;
        }
    }
    if let Some(e) = v.get("effect").and_then(parse_effect) {
        l.effect = e;
    }
    if let Some(m) = v.get("movement").and_then(parse_movement) {
        l.movement = m;
    }
    if let Some(x) = f("rate") {
        l.rate = x.clamp(0.125, 32.0);
    }
    if let Some(x) = f("move_size") {
        l.move_size = x.clamp(0.0, 1.0);
    }
    if let Some(x) = f("move_rate") {
        l.move_rate = x.clamp(0.25, 64.0);
    }
    if let Some(x) = f("tilt") {
        l.tilt = x.clamp(-90.0, 90.0);
    }
    if let Some(x) = f("spread") {
        l.spread = x.clamp(0.0, 1.0);
    }
    if let Some(x) = f("strobe") {
        l.strobe = x.clamp(0.0, 1.0);
    }
    if let Some(m) = v.get("mirror").and_then(|x| x.as_bool()) {
        l.mirror = m;
    }
    l
}

pub fn parse_claude(text: &str, song: &Song) -> Result<(Vec<Cue>, Vec<Hit>), String> {
    let a = text.find('{').ok_or("La risposta di Claude non contiene uno show")?;
    let z = text.rfind('}').ok_or("La risposta di Claude non contiene uno show")?;
    let v: Value = serde_json::from_str(&text[a..=z]).map_err(|e| format!("Risposta di Claude non valida: {e}"))?;
    let ana = song.analysis.as_ref();
    let dur = song.duration();
    let mut cues = vec![];
    for c in v.get("cues").and_then(|c| c.as_array()).cloned().unwrap_or_default() {
        let mut t = c.get("time").and_then(|x| x.as_f64()).unwrap_or(0.0).clamp(0.0, dur);
        if let Some(a) = ana {
            if t > 0.05 {
                t = a.snap(t, 1.0);
            }
        }
        let mut cue = Cue::blank(t, c.get("name").and_then(|x| x.as_str()).unwrap_or("Cue"));
        cue.fade = c.get("fade").and_then(|x| x.as_f64()).unwrap_or(0.3).clamp(0.0, 10.0);
        if let Some(obj) = c.get("looks").and_then(|l| l.as_object()) {
            for (k, lv) in obj {
                if let Some(g) = Group::ALL.iter().find(|g| format!("{g:?}").eq_ignore_ascii_case(k)) {
                    cue.looks[g.idx()] = parse_look(lv);
                }
            }
        }
        cues.push(cue);
    }
    if cues.is_empty() {
        return Err("Claude non ha prodotto nessuna scena".into());
    }
    let mut hits = vec![];
    for h in v.get("hits").and_then(|c| c.as_array()).cloned().unwrap_or_default() {
        let t = h.get("time").and_then(|x| x.as_f64()).unwrap_or(-1.0);
        if !(0.0..=dur).contains(&t) {
            continue;
        }
        let k = h.get("kind").and_then(|x| x.as_str()).unwrap_or("").to_lowercase();
        let kind = if k.contains("blind") {
            HitKind::Blinder
        } else if k.contains("strob") {
            HitKind::Strobe
        } else if k.contains("black") {
            HitKind::Blackout
        } else {
            HitKind::Flash
        };
        let d = h.get("duration").and_then(|x| x.as_f64()).unwrap_or(0.2).clamp(0.02, 8.0);
        hits.push(Hit { time: t, kind, duration: d });
    }
    cues.sort_by(|x, y| x.time.total_cmp(&y.time));
    hits.sort_by(|x, y| x.time.total_cmp(&y.time));
    Ok((cues, hits))
}
