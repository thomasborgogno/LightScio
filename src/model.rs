//! Data model: fixtures, rig, songs, analysis results and the editable show.

use crate::i18n::ts;
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------- fixtures

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Debug, Hash)]
pub enum ChannelKind {
    Dimmer,
    Red,
    Green,
    Blue,
    White,
    Amber,
    Uv,
    Pan,
    PanFine,
    Tilt,
    TiltFine,
    PanTiltSpeed,
    Shutter,
    Strobe,
    ColorWheel,
    Gobo,
    Prism,
    Focus,
    Zoom,
    Fixed,
    Other,
}

impl ChannelKind {
    pub const ALL: [ChannelKind; 21] = [
        ChannelKind::Dimmer,
        ChannelKind::Red,
        ChannelKind::Green,
        ChannelKind::Blue,
        ChannelKind::White,
        ChannelKind::Amber,
        ChannelKind::Uv,
        ChannelKind::Pan,
        ChannelKind::PanFine,
        ChannelKind::Tilt,
        ChannelKind::TiltFine,
        ChannelKind::PanTiltSpeed,
        ChannelKind::Shutter,
        ChannelKind::Strobe,
        ChannelKind::ColorWheel,
        ChannelKind::Gobo,
        ChannelKind::Prism,
        ChannelKind::Focus,
        ChannelKind::Zoom,
        ChannelKind::Fixed,
        ChannelKind::Other,
    ];
    pub fn label(self) -> &'static str {
        match self {
            ChannelKind::Dimmer => ts("Dimmer (intensità)"),
            ChannelKind::Red => ts("Rosso"),
            ChannelKind::Green => ts("Verde"),
            ChannelKind::Blue => ts("Blu"),
            ChannelKind::White => ts("Bianco"),
            ChannelKind::Amber => ts("Ambra"),
            ChannelKind::Uv => ts("UV"),
            ChannelKind::Pan => ts("Pan"),
            ChannelKind::PanFine => ts("Pan fine"),
            ChannelKind::Tilt => ts("Tilt"),
            ChannelKind::TiltFine => ts("Tilt fine"),
            ChannelKind::PanTiltSpeed => ts("Velocità pan/tilt"),
            ChannelKind::Shutter => ts("Shutter / strobo"),
            ChannelKind::Strobe => ts("Strobo (0 = spento)"),
            ChannelKind::ColorWheel => ts("Ruota colori"),
            ChannelKind::Gobo => ts("Gobo"),
            ChannelKind::Prism => ts("Prisma"),
            ChannelKind::Focus => ts("Fuoco"),
            ChannelKind::Zoom => ts("Zoom"),
            ChannelKind::Fixed => ts("Valore fisso"),
            ChannelKind::Other => ts("Altro"),
        }
    }
}

#[derive(Clone, Serialize, Deserialize, Debug, PartialEq)]
pub struct ChannelDef {
    pub name: String,
    pub kind: ChannelKind,
    #[serde(default)]
    pub default: u8,
    /// What the channel does over its DMX range (from the fixture library).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub caps: Vec<CapInfo>,
}

pub fn ch(name: &str, kind: ChannelKind, default: u8) -> ChannelDef {
    ChannelDef { name: crate::i18n::t(name), kind, default, caps: vec![] }
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Debug, Hash)]
pub enum FixtureKind {
    Par,
    Blinder,
    Strobe,
    MovingSpot,
    MovingBeam,
    MovingWash,
    LedBar,
    Other,
}

impl FixtureKind {
    pub const ALL: [FixtureKind; 8] = [
        FixtureKind::Par,
        FixtureKind::Blinder,
        FixtureKind::Strobe,
        FixtureKind::MovingSpot,
        FixtureKind::MovingBeam,
        FixtureKind::MovingWash,
        FixtureKind::LedBar,
        FixtureKind::Other,
    ];
    pub fn label(self) -> &'static str {
        match self {
            FixtureKind::Par => ts("PAR LED"),
            FixtureKind::Blinder => ts("Accecatore (blinder)"),
            FixtureKind::Strobe => ts("Strobo"),
            FixtureKind::MovingSpot => ts("Testa mobile Spot"),
            FixtureKind::MovingBeam => ts("Testa mobile Beam"),
            FixtureKind::MovingWash => ts("Testa mobile Wash"),
            FixtureKind::LedBar => ts("Barra LED"),
            FixtureKind::Other => ts("Altro"),
        }
    }
    pub fn is_moving(self) -> bool {
        matches!(self, FixtureKind::MovingSpot | FixtureKind::MovingBeam | FixtureKind::MovingWash)
    }
}

#[derive(Clone, Serialize, Deserialize, Debug, PartialEq)]
pub struct ColorSlot {
    pub value: u8,
    pub color: [u8; 3],
}

#[derive(Clone, Serialize, Deserialize, Debug, PartialEq)]
pub struct FixtureDef {
    pub id: String,
    pub name: String,
    pub kind: FixtureKind,
    pub channels: Vec<ChannelDef>,
    #[serde(default = "d540")]
    pub pan_range: f32,
    #[serde(default = "d270")]
    pub tilt_range: f32,
    #[serde(default)]
    pub color_wheel: Vec<ColorSlot>,
    #[serde(default = "d25")]
    pub beam_angle: f32,
    /// DMX value of the Shutter channel for "open beam".
    #[serde(default = "d255")]
    pub shutter_open: u8,
    #[serde(default)]
    pub shutter_closed: u8,
    /// Strobe range (slow..fast) on the Shutter / Strobe channel.
    #[serde(default = "d10")]
    pub strobe_min: u8,
    #[serde(default = "d250")]
    pub strobe_max: u8,
    #[serde(default)]
    pub custom: bool,
    /// Wheels (colours, gobos, …) with their slots (from the fixture library).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub wheels: Vec<WheelInfo>,
    /// Data sheet: manufacturer, physical data, links, source (from the fixture library).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub info: Option<Box<FixtureInfo>>,
}

/// One function of a channel over a DMX range.
#[derive(Clone, Serialize, Deserialize, Debug, PartialEq, Default)]
pub struct CapInfo {
    pub start: u8,
    pub end: u8,
    /// Function type as named by the Open Fixture Library ("ShutterStrobe", "WheelSlot", …).
    pub typ: String,
    /// What exactly: shutter effect, colour, wheel slot, effect name, …
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub what: String,
    /// Values over the range, e.g. "1Hz → 12Hz", "0° → 540°".
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub values: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub comment: String,
    /// Colours shown by this range (colour presets, colour wheel slots).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub colors: Vec<[u8; 3]>,
}

#[derive(Clone, Serialize, Deserialize, Debug, PartialEq, Default)]
pub struct WheelInfo {
    pub name: String,
    pub slots: Vec<SlotInfo>,
}

#[derive(Clone, Serialize, Deserialize, Debug, PartialEq, Default)]
pub struct SlotInfo {
    /// "Open", "Color", "Gobo", "Prism", …
    pub typ: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub colors: Vec<[u8; 3]>,
    /// Extra data (colour temperature, prism facets, gobo resource, …).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub detail: String,
}

/// Data sheet of a library fixture, for the given DMX mode.
#[derive(Clone, Serialize, Deserialize, Debug, PartialEq, Default)]
#[serde(default)]
pub struct FixtureInfo {
    /// e.g. "Open Fixture Library"
    pub library: String,
    /// Library key, e.g. "clay-paky/sharpy".
    pub key: String,
    /// Library version (commit) the data comes from.
    pub version: String,
    pub manufacturer: String,
    pub model: String,
    pub short_name: String,
    pub categories: Vec<String>,
    pub mode: String,
    pub mode_short: String,
    pub comment: String,
    /// Width, height, depth in mm.
    pub dimensions: Option<[f32; 3]>,
    /// kg
    pub weight: Option<f32>,
    /// W
    pub power: Option<f32>,
    pub dmx_connector: String,
    pub bulb: String,
    /// K
    pub color_temperature: Option<f32>,
    pub lumens: Option<f32>,
    pub lens: String,
    /// Beam angle range in degrees.
    pub lens_degrees: Option<[f32; 2]>,
    pub focus_type: String,
    /// Pan / tilt range as written by the library ("540°", "infinito").
    pub pan_max: String,
    pub tilt_max: String,
    /// Pixel matrix (x, y, z) for LED bars / matrices.
    pub matrix: Option<[u32; 3]>,
    /// (kind, url): manual, product page, video, …
    pub links: Vec<(String, String)>,
    pub authors: Vec<String>,
    pub created: String,
    pub modified: String,
}

fn d540() -> f32 {
    540.0
}
fn d270() -> f32 {
    270.0
}
fn d25() -> f32 {
    25.0
}
fn d255() -> u8 {
    255
}
fn d10() -> u8 {
    10
}
fn d250() -> u8 {
    250
}

impl FixtureDef {
    pub fn has(&self, k: ChannelKind) -> bool {
        self.channels.iter().any(|c| c.kind == k)
    }
    pub fn footprint(&self) -> u16 {
        self.channels.len().max(1) as u16
    }
    pub fn has_rgb(&self) -> bool {
        self.has(ChannelKind::Red) || self.has(ChannelKind::Green) || self.has(ChannelKind::Blue)
    }
    pub fn capabilities(&self) -> String {
        let mut v = vec![];
        if self.has(ChannelKind::Dimmer) {
            v.push(ts("dimmer"));
        }
        if self.has_rgb() {
            v.push(ts("colori RGB"));
        }
        if self.has(ChannelKind::White) {
            v.push(ts("bianco"));
        }
        if self.has(ChannelKind::ColorWheel) {
            v.push(ts("ruota colori"));
        }
        if self.has(ChannelKind::Pan) || self.has(ChannelKind::Tilt) {
            v.push(ts("movimento pan/tilt"));
        }
        if self.has(ChannelKind::Shutter) || self.has(ChannelKind::Strobe) {
            v.push(ts("strobo"));
        }
        if self.has(ChannelKind::Gobo) {
            v.push(ts("gobo"));
        }
        if self.has(ChannelKind::Prism) {
            v.push(ts("prisma"));
        }
        v.join(", ")
    }
}

/// Logical groups the show is programmed on.
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Debug, Hash)]
pub enum Group {
    FrontWash,
    BackWash,
    Blinders,
    HeadsFront,
    HeadsBack,
    Strobes,
    Bars,
    Extra,
}

impl Group {
    pub const ALL: [Group; 8] = [
        Group::FrontWash,
        Group::BackWash,
        Group::Blinders,
        Group::HeadsFront,
        Group::HeadsBack,
        Group::Strobes,
        Group::Bars,
        Group::Extra,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Group::FrontWash => ts("Wash frontale"),
            Group::BackWash => ts("Controluce (PAR dietro)"),
            Group::Blinders => ts("Accecatori"),
            Group::HeadsFront => ts("Teste mobili davanti"),
            Group::HeadsBack => ts("Teste mobili dietro"),
            Group::Strobes => ts("Strobo"),
            Group::Bars => ts("Barre LED"),
            Group::Extra => ts("Altro"),
        }
    }
    pub fn idx(self) -> usize {
        Group::ALL.iter().position(|g| *g == self).unwrap_or(0)
    }
}

#[derive(Clone, Serialize, Deserialize, Debug, PartialEq)]
pub struct FixtureInstance {
    pub id: u32,
    pub def_id: String,
    pub name: String,
    /// DMX start address 1..=512
    pub address: u16,
    /// Position in meters: x = left(-)/right(+) seen from the audience,
    /// y = depth (0 = front edge, grows towards the back), z = height.
    pub x: f32,
    pub y: f32,
    pub z: f32,
    /// Hanging upside down from a truss.
    pub hanging: bool,
    pub group: Group,
    /// Static aim (for non-moving fixtures) / home pan for moving heads, degrees.
    #[serde(default)]
    pub aim_pan: f32,
    /// Degrees away from the vertical, positive = towards the audience.
    #[serde(default)]
    pub aim_tilt: f32,
}

#[derive(Clone, Serialize, Deserialize, Debug, PartialEq)]
pub struct Stage {
    pub width: f32,
    pub depth: f32,
    pub truss_front: f32,
    pub truss_back: f32,
}

impl Default for Stage {
    fn default() -> Self {
        Stage { width: 10.0, depth: 6.0, truss_front: 4.5, truss_back: 5.0 }
    }
}

// ---------------------------------------------------------------- songs

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Debug, Hash)]
pub enum TrackRole {
    Drums,
    Kick,
    Snare,
    Bass,
    Guitar,
    Keys,
    Vocals,
    Click,
    Other,
    Ignore,
}

impl TrackRole {
    pub const ALL: [TrackRole; 10] = [
        TrackRole::Drums,
        TrackRole::Kick,
        TrackRole::Snare,
        TrackRole::Bass,
        TrackRole::Guitar,
        TrackRole::Keys,
        TrackRole::Vocals,
        TrackRole::Click,
        TrackRole::Other,
        TrackRole::Ignore,
    ];
    pub fn label(self) -> &'static str {
        match self {
            TrackRole::Drums => ts("Batteria"),
            TrackRole::Kick => ts("Cassa"),
            TrackRole::Snare => ts("Rullante"),
            TrackRole::Bass => ts("Basso"),
            TrackRole::Guitar => ts("Chitarre"),
            TrackRole::Keys => ts("Tastiere / synth"),
            TrackRole::Vocals => ts("Voci / cori"),
            TrackRole::Click => ts("Click / guida"),
            TrackRole::Other => ts("Altro strumento"),
            TrackRole::Ignore => ts("Ignora"),
        }
    }
    pub fn is_drum(self) -> bool {
        matches!(self, TrackRole::Drums | TrackRole::Kick | TrackRole::Snare)
    }
    /// Guess the role from a file name (Italian and English words).
    pub fn guess(name: &str) -> TrackRole {
        let n = name.to_lowercase();
        let has = |w: &[&str]| w.iter().any(|x| n.contains(x));
        if has(&["click", "metro", "guide", "guida", "cue", "count"]) {
            TrackRole::Click
        } else if has(&["kick", "cassa", "bd"]) {
            TrackRole::Kick
        } else if has(&["snare", "rullante", "sn"]) && !has(&["synth"]) {
            TrackRole::Snare
        } else if has(&["drum", "batt", "perc", "loop", "beat", "ovh", "overhead", "tom", "hat"]) {
            TrackRole::Drums
        } else if has(&["bass", "basso", "sub"]) {
            TrackRole::Bass
        } else if has(&["gtr", "guit", "chit"]) {
            TrackRole::Guitar
        } else if has(&["key", "synth", "pad", "piano", "organ", "tast", "string", "archi"]) {
            TrackRole::Keys
        } else if has(&["vox", "voc", "voce", "cori", "choir", "bv", "lead"]) {
            TrackRole::Vocals
        } else {
            TrackRole::Other
        }
    }
}

#[derive(Clone, Serialize, Deserialize, Debug, PartialEq)]
pub struct TrackInfo {
    pub path: String,
    pub name: String,
    pub role: TrackRole,
    /// Loudness envelope at ENV_FPS, 0..1 (for drawing).
    #[serde(default)]
    pub env: Vec<f32>,
    /// Hidden in the timelines.
    #[serde(default)]
    pub hidden: bool,
}

pub const ENV_FPS: f64 = 20.0;

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Debug, Hash)]
pub enum SectionKind {
    Intro,
    Verse,
    PreChorus,
    Chorus,
    Bridge,
    Solo,
    Break,
    Outro,
    Other,
}

impl SectionKind {
    pub const ALL: [SectionKind; 9] = [
        SectionKind::Intro,
        SectionKind::Verse,
        SectionKind::PreChorus,
        SectionKind::Chorus,
        SectionKind::Bridge,
        SectionKind::Solo,
        SectionKind::Break,
        SectionKind::Outro,
        SectionKind::Other,
    ];
    pub fn label(self) -> &'static str {
        match self {
            SectionKind::Intro => ts("Intro"),
            SectionKind::Verse => ts("Strofa"),
            SectionKind::PreChorus => ts("Pre-ritornello"),
            SectionKind::Chorus => ts("Ritornello"),
            SectionKind::Bridge => ts("Bridge / special"),
            SectionKind::Solo => ts("Assolo"),
            SectionKind::Break => ts("Stacco / break"),
            SectionKind::Outro => ts("Finale"),
            SectionKind::Other => ts("Altro"),
        }
    }
    pub fn color(self) -> [u8; 3] {
        match self {
            SectionKind::Intro => [70, 90, 160],
            SectionKind::Verse => [50, 130, 150],
            SectionKind::PreChorus => [150, 130, 50],
            SectionKind::Chorus => [180, 60, 70],
            SectionKind::Bridge => [130, 70, 160],
            SectionKind::Solo => [200, 110, 40],
            SectionKind::Break => [90, 90, 90],
            SectionKind::Outro => [60, 70, 120],
            SectionKind::Other => [80, 100, 80],
        }
    }
}

pub fn intensity_label(i: f32) -> &'static str {
    if i < 0.2 {
        ts("Calmo")
    } else if i < 0.4 {
        ts("Morbido")
    } else if i < 0.6 {
        ts("Medio")
    } else if i < 0.8 {
        ts("Energico")
    } else {
        ts("Esplosivo")
    }
}

#[derive(Clone, Serialize, Deserialize, Debug, PartialEq)]
pub struct Section {
    pub start: f64,
    pub end: f64,
    pub kind: SectionKind,
    pub label: String,
    /// 0..1
    pub intensity: f32,
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Debug, Hash)]
pub enum HitKind {
    Blinder,
    Strobe,
    Flash,
    Blackout,
}

impl HitKind {
    pub const ALL: [HitKind; 4] = [HitKind::Blinder, HitKind::Strobe, HitKind::Flash, HitKind::Blackout];
    pub fn label(self) -> &'static str {
        match self {
            HitKind::Blinder => ts("Accecatori"),
            HitKind::Strobe => ts("Strobo"),
            HitKind::Flash => ts("Flash bianco totale"),
            HitKind::Blackout => ts("Blackout (stacco)"),
        }
    }
    pub fn color(self) -> [u8; 3] {
        match self {
            HitKind::Blinder => [255, 200, 120],
            HitKind::Strobe => [230, 230, 255],
            HitKind::Flash => [255, 255, 255],
            HitKind::Blackout => [40, 40, 40],
        }
    }
}

#[derive(Clone, Serialize, Deserialize, Debug, PartialEq)]
pub struct Hit {
    pub time: f64,
    pub kind: HitKind,
    pub duration: f64,
}

/// What the audio analysis found.
#[derive(Clone, Serialize, Deserialize, Debug, PartialEq, Default)]
pub struct Analysis {
    pub bpm: f64,
    /// Time of the first downbeat (seconds).
    pub offset: f64,
    pub beats_per_bar: u32,
    pub duration: f64,
    /// Overall loudness envelope at ENV_FPS (0..1).
    pub env: Vec<f32>,
    pub kicks: Vec<f64>,
    pub snares: Vec<f64>,
    /// Accents found (stacchi / colpi d'insieme), seconds + strength.
    pub accents: Vec<(f64, f32)>,
    /// Stops: (start, duration).
    pub stops: Vec<(f64, f64)>,
    pub character: String,
    pub mean_intensity: f32,
    #[serde(default)]
    pub used_click: bool,
    /// Extra downbeats placed by the user: from each one the bars restart,
    /// so the bar just before it can be shorter (e.g. a 2/4 bar).
    #[serde(default)]
    pub downbeats: Vec<f64>,
}

impl Analysis {
    pub fn spb(&self) -> f64 {
        60.0 / self.bpm.max(1.0)
    }
    pub fn bar_len(&self) -> f64 {
        self.spb() * self.beats_per_bar.max(1) as f64
    }
    /// First downbeat plus the extra downbeats, sorted.
    pub fn anchors(&self) -> Vec<f64> {
        let mut v = vec![self.offset];
        for &d in &self.downbeats {
            if d > self.offset + 0.05 && d < self.duration.max(self.offset + 0.1) {
                v.push(d);
            }
        }
        v.sort_by(|a, b| a.total_cmp(b));
        v.dedup_by(|a, b| (*a - *b).abs() < 0.02);
        v
    }
    /// Beat number at which each anchor starts.
    fn bases(&self, anchors: &[f64]) -> Vec<f64> {
        let spb = self.spb();
        let mut b = vec![0.0];
        for k in 1..anchors.len() {
            let prev = b[k - 1];
            b.push(prev + ((anchors[k] - anchors[k - 1]) / spb).round().max(1.0));
        }
        b
    }
    fn seg(anchors: &[f64], t: f64) -> usize {
        anchors.partition_point(|&a| a <= t).saturating_sub(1)
    }
    /// Beat position (in beats, from the first downbeat) at time t.
    pub fn beat_at(&self, t: f64) -> f64 {
        let a = self.anchors();
        let b = self.bases(&a);
        let k = Self::seg(&a, t);
        b[k] + (t - a[k]) / self.spb()
    }
    pub fn time_of_beat(&self, beat: f64) -> f64 {
        let a = self.anchors();
        let b = self.bases(&a);
        let k = b.partition_point(|&x| x <= beat).saturating_sub(1);
        a[k] + (beat - b[k]) * self.spb()
    }
    /// Snap to the nearest 1/div of a beat (respecting the extra downbeats).
    pub fn snap(&self, t: f64, div: f64) -> f64 {
        let a = self.anchors();
        let k = Self::seg(&a, t);
        let spb = self.spb();
        let local = ((t - a[k]) / spb * div).round() / div;
        let mut r = a[k] + local * spb;
        if let Some(&next) = a.get(k + 1) {
            if r > next || (t - next).abs() < (t - r).abs() {
                r = next;
            }
        }
        r.max(0.0)
    }
    /// Start times of all bars (from the first downbeat to the end).
    pub fn bar_starts(&self) -> Vec<f64> {
        let a = self.anchors();
        let bar = self.bar_len().max(0.05);
        let end_all = self.duration.max(self.offset + bar);
        let mut out = vec![];
        for k in 0..a.len() {
            let end = a.get(k + 1).copied().unwrap_or(end_all);
            let mut t = a[k];
            while t < end - 1e-3 && out.len() < 20000 {
                out.push(t);
                t += bar;
            }
        }
        out
    }
    /// All beats as (time, is_downbeat).
    pub fn beat_times(&self) -> Vec<(f64, bool)> {
        let starts = self.bar_starts();
        let spb = self.spb().max(0.05);
        let end_all = self.duration.max(self.offset);
        let mut out = vec![];
        for (i, &s) in starts.iter().enumerate() {
            let next = starts.get(i + 1).copied().unwrap_or(end_all);
            let mut t = s;
            let mut first = true;
            while t < next - 0.01 {
                out.push((t, first));
                first = false;
                t += spb;
            }
        }
        out
    }
    /// Nearest bar start.
    pub fn snap_bar(&self, t: f64) -> f64 {
        let s = self.bar_starts();
        s.iter().copied().min_by(|a, b| (a - t).abs().total_cmp(&(b - t).abs())).unwrap_or(t)
    }
    /// 1-based number of the bar containing t (0 before the first downbeat).
    pub fn bar_number(&self, t: f64) -> i64 {
        self.bar_starts().partition_point(|&s| s <= t + 1e-6) as i64
    }
    /// "bar.beat" label (1-based).
    pub fn bar_beat(&self, t: f64) -> String {
        let s = self.bar_starts();
        let i = s.partition_point(|&x| x <= t + 1e-6);
        if i == 0 {
            let before = ((self.offset - t) / self.spb()).ceil() as i64;
            return format!("0.-{}", before.max(1));
        }
        let beat = ((t - s[i - 1]) / self.spb() + 1e-6).floor().max(0.0) as i64 + 1;
        format!("{}.{}", i, beat)
    }
}

/// Heights and visibility of the timeline lanes (saved with the project).
#[derive(Clone, Serialize, Deserialize, Debug, PartialEq)]
#[serde(default)]
pub struct ViewSettings {
    pub sec_h: f32,
    pub cue_h: f32,
    pub hit_h: f32,
    pub energy_h: f32,
    pub track_h: f32,
    pub show_energy: bool,
    pub stage_frac: f32,
}

impl Default for ViewSettings {
    fn default() -> Self {
        ViewSettings { sec_h: 28.0, cue_h: 30.0, hit_h: 24.0, energy_h: 44.0, track_h: 26.0, show_energy: true, stage_frac: 0.46 }
    }
}

// ---------------------------------------------------------------- show

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Debug, Hash)]
pub enum Effect {
    Static,
    Pulse,
    Kick,
    Snare,
    Chase,
    Alternate,
    Breath,
    Sparkle,
    ColorChase,
    Off,
}

impl Effect {
    pub const ALL: [Effect; 10] = [
        Effect::Static,
        Effect::Pulse,
        Effect::Kick,
        Effect::Snare,
        Effect::Chase,
        Effect::Alternate,
        Effect::Breath,
        Effect::Sparkle,
        Effect::ColorChase,
        Effect::Off,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Effect::Static => ts("Fisso"),
            Effect::Pulse => ts("Pulsa a tempo"),
            Effect::Kick => ts("Segue la cassa"),
            Effect::Snare => ts("Segue il rullante"),
            Effect::Chase => ts("Rincorsa (chase)"),
            Effect::Alternate => ts("Alternato pari/dispari"),
            Effect::Breath => ts("Respiro lento"),
            Effect::Sparkle => ts("Scintillio casuale"),
            Effect::ColorChase => ts("Giro di colori"),
            Effect::Off => ts("Spento"),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Debug, Hash)]
pub enum Movement {
    Still,
    Circle,
    Figure8,
    PanSweep,
    TiltSweep,
    Ballyhoo,
    Crowd,
    Fan,
}

impl Movement {
    pub const ALL: [Movement; 8] = [
        Movement::Still,
        Movement::Circle,
        Movement::Figure8,
        Movement::PanSweep,
        Movement::TiltSweep,
        Movement::Ballyhoo,
        Movement::Crowd,
        Movement::Fan,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Movement::Still => ts("Fermo"),
            Movement::Circle => ts("Cerchio"),
            Movement::Figure8 => ts("Otto"),
            Movement::PanSweep => ts("Spazzata orizzontale"),
            Movement::TiltSweep => ts("Spazzata verticale"),
            Movement::Ballyhoo => ts("Ballyhoo (caotico)"),
            Movement::Crowd => ts("Sul pubblico"),
            Movement::Fan => ts("Ventaglio"),
        }
    }
}

#[derive(Clone, Serialize, Deserialize, Debug, PartialEq)]
#[serde(default)]
pub struct GroupLook {
    pub intensity: f32,
    pub colors: Vec<[u8; 3]>,
    pub effect: Effect,
    /// Beats per effect cycle.
    pub rate: f32,
    pub movement: Movement,
    pub move_size: f32,
    /// Beats per movement cycle.
    pub move_rate: f32,
    /// Center tilt (degrees from vertical, + = towards audience).
    pub tilt: f32,
    /// Phase spread across fixtures 0..1.
    pub spread: f32,
    /// 0 = off, 1 = fastest strobe.
    pub strobe: f32,
    /// Moving heads on the left of the stage do the movement mirrored.
    pub mirror: bool,
}

impl Default for GroupLook {
    fn default() -> Self {
        GroupLook {
            intensity: 0.0,
            colors: vec![[255, 255, 255]],
            effect: Effect::Static,
            rate: 1.0,
            movement: Movement::Still,
            move_size: 0.4,
            move_rate: 8.0,
            tilt: 35.0,
            spread: 0.5,
            strobe: 0.0,
            mirror: false,
        }
    }
}

#[derive(Clone, Serialize, Deserialize, Debug, PartialEq)]
pub struct Cue {
    pub time: f64,
    pub name: String,
    pub fade: f64,
    pub looks: Vec<GroupLook>,
}

impl Cue {
    pub fn blank(time: f64, name: &str) -> Cue {
        Cue { time, name: name.to_string(), fade: 0.3, looks: vec![GroupLook::default(); Group::ALL.len()] }
    }
    pub fn fix(&mut self) {
        self.looks.resize(Group::ALL.len(), GroupLook::default());
        for l in &mut self.looks {
            if l.colors.is_empty() {
                l.colors.push([255, 255, 255]);
            }
        }
    }
}

#[derive(Clone, Serialize, Deserialize, Debug, PartialEq)]
pub struct Song {
    pub name: String,
    pub tracks: Vec<TrackInfo>,
    pub analysis: Option<Analysis>,
    pub sections: Vec<Section>,
    pub cues: Vec<Cue>,
    pub hits: Vec<Hit>,
    /// MIDI Program Change that selects this song in live (0..127).
    pub program: u8,
}

impl Song {
    pub fn new(name: &str, program: u8) -> Song {
        Song {
            name: name.to_string(),
            tracks: vec![],
            analysis: None,
            sections: vec![],
            cues: vec![],
            hits: vec![],
            program,
        }
    }
    pub fn duration(&self) -> f64 {
        self.analysis.as_ref().map(|a| a.duration).unwrap_or(0.0).max(1.0)
    }
    pub fn sort(&mut self) {
        self.cues.sort_by(|a, b| a.time.total_cmp(&b.time));
        self.hits.sort_by(|a, b| a.time.total_cmp(&b.time));
        self.sections.sort_by(|a, b| a.start.total_cmp(&b.start));
    }
    pub fn section_at(&self, t: f64) -> Option<usize> {
        self.sections.iter().position(|s| t >= s.start && t < s.end)
    }
}

// ---------------------------------------------------------------- project

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Debug)]
pub enum DmxProtocol {
    OpenDmx,
    EnttecPro,
}

impl DmxProtocol {
    pub fn label(self) -> &'static str {
        match self {
            DmxProtocol::OpenDmx => ts("Open DMX / FTDI (dongle economici)"),
            DmxProtocol::EnttecPro => ts("Compatibile Enttec DMX USB Pro"),
        }
    }
}

#[derive(Clone, Serialize, Deserialize, Debug, PartialEq)]
#[serde(default)]
pub struct Settings {
    /// 0 = any channel, 1..=16
    pub midi_channel: u8,
    pub dmx_protocol: DmxProtocol,
    pub dmx_fps: u32,
    pub claude_command: String,
    pub stop_note_at_end: bool,
    pub master: f32,
    /// Last MIDI input port used (remembered with the project).
    pub midi_port: String,
    /// Last DMX interface port used.
    pub dmx_port: String,
    /// Opening the project goes straight to Live and connects MIDI/DMX by itself.
    pub auto_live: bool,
    /// If the remembered MIDI port is missing, use the first available one.
    pub any_midi_port: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            midi_channel: 16,
            dmx_protocol: DmxProtocol::OpenDmx,
            dmx_fps: 40,
            claude_command: "claude".to_string(),
            stop_note_at_end: true,
            master: 1.0,
            midi_port: String::new(),
            dmx_port: String::new(),
            auto_live: false,
            any_midi_port: true,
        }
    }
}

#[derive(Clone, Serialize, Deserialize, Debug, PartialEq)]
pub struct Project {
    pub name: String,
    pub stage: Stage,
    pub custom_defs: Vec<FixtureDef>,
    pub fixtures: Vec<FixtureInstance>,
    pub songs: Vec<Song>,
    #[serde(default)]
    pub settings: Settings,
    #[serde(default)]
    pub next_id: u32,
    #[serde(default)]
    pub view: ViewSettings,
}

/// A lighting setup on its own (to convert a show to other lights).
#[derive(Clone, Serialize, Deserialize, Debug, PartialEq)]
pub struct Rig {
    pub stage: Stage,
    pub custom_defs: Vec<FixtureDef>,
    pub fixtures: Vec<FixtureInstance>,
}

impl Default for Project {
    fn default() -> Self {
        Project {
            name: "Nuovo show".into(),
            stage: Stage::default(),
            custom_defs: vec![],
            fixtures: vec![],
            songs: vec![],
            settings: Settings::default(),
            next_id: 1,
            view: ViewSettings::default(),
        }
    }
}

impl Project {
    pub fn fix(&mut self) {
        for s in &mut self.songs {
            for c in &mut s.cues {
                c.fix();
            }
            s.sort();
        }
        let max = self.fixtures.iter().map(|f| f.id).max().unwrap_or(0);
        if self.next_id <= max {
            self.next_id = max + 1;
        }
    }
    pub fn def<'a>(&'a self, catalog: &'a [FixtureDef], id: &str) -> Option<&'a FixtureDef> {
        self.custom_defs.iter().find(|d| d.id == id).or_else(|| catalog.iter().find(|d| d.id == id))
    }
    /// First free DMX address after all patched fixtures.
    pub fn next_address(&self, catalog: &[FixtureDef]) -> u16 {
        let mut end = 1u16;
        for f in &self.fixtures {
            let n = self.def(catalog, &f.id_def()).map(|d| d.footprint()).unwrap_or(1);
            end = end.max(f.address + n);
        }
        end.min(512)
    }
    pub fn group_count(&self, g: Group) -> usize {
        self.fixtures.iter().filter(|f| f.group == g).count()
    }
}

impl FixtureInstance {
    pub fn id_def(&self) -> String {
        self.def_id.clone()
    }
}

/// Default group for a fixture kind at a position.
pub fn auto_group(kind: FixtureKind, y: f32, depth: f32) -> Group {
    let back = y > depth * 0.45;
    match kind {
        FixtureKind::Blinder => Group::Blinders,
        FixtureKind::Strobe => Group::Strobes,
        FixtureKind::LedBar => Group::Bars,
        FixtureKind::Par => {
            if back {
                Group::BackWash
            } else {
                Group::FrontWash
            }
        }
        FixtureKind::MovingBeam | FixtureKind::MovingSpot | FixtureKind::MovingWash => {
            if back {
                Group::HeadsBack
            } else {
                Group::HeadsFront
            }
        }
        FixtureKind::Other => Group::Extra,
    }
}
