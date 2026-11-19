//! Import of QLC+ fixture definitions (.qxf files) from a folder chosen by the user.

use crate::model::ChannelKind as K;
use crate::model::*;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub struct QlcEntry {
    pub manufacturer: String,
    pub model: String,
    pub kind: String,
    pub path: PathBuf,
    /// lowercase "manufacturer model type" for searching
    pub key: String,
}

/// Folders where QLC+ fixtures are usually found.
pub fn default_dirs() -> Vec<PathBuf> {
    let mut v = vec![
        PathBuf::from(r"C:\QLC+\Fixtures"),
        PathBuf::from(r"C:\Program Files\QLC+\Fixtures"),
        PathBuf::from(r"C:\Program Files (x86)\QLC+\Fixtures"),
        PathBuf::from("/Applications/QLC+.app/Contents/Resources/Fixtures"),
    ];
    if let Some(home) = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")) {
        let h = PathBuf::from(home);
        v.push(h.join("QLC+").join("Fixtures"));
        v.push(h.join("Library/Application Support/QLC+/Fixtures"));
    }
    v.into_iter().filter(|p| p.is_dir()).collect()
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>, depth: usize) {
    if depth > 8 {
        return;
    }
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            let n = p.file_name().map(|s| s.to_string_lossy().to_lowercase()).unwrap_or_default();
            if n.starts_with('.') || n == "target" || n == "node_modules" {
                continue;
            }
            walk(&p, out, depth + 1);
        } else if p.extension().map(|x| x.eq_ignore_ascii_case("qxf")).unwrap_or(false) {
            out.push(p);
        }
    }
}

fn unescape(s: &str) -> String {
    s.replace("&amp;", "&").replace("&apos;", "'").replace("&quot;", "\"").replace("&lt;", "<").replace("&gt;", ">")
}

fn tag(text: &str, name: &str) -> Option<String> {
    let open = format!("<{name}>");
    let close = format!("</{name}>");
    let a = text.find(&open)? + open.len();
    let b = text[a..].find(&close)? + a;
    Some(unescape(text[a..b].trim()))
}

/// Quick index of all fixtures (reads only manufacturer / model / type).
pub fn scan(dirs: &[PathBuf]) -> Vec<QlcEntry> {
    let mut files = vec![];
    for d in dirs {
        walk(d, &mut files, 0);
    }
    files.sort();
    files.dedup();
    let mut out = vec![];
    for p in files {
        let Ok(text) = std::fs::read_to_string(&p) else { continue };
        let head = &text[..text.len().min(4000)];
        let (Some(manufacturer), Some(model)) = (tag(head, "Manufacturer"), tag(head, "Model")) else { continue };
        let kind = tag(head, "Type").unwrap_or_default();
        let key = format!("{manufacturer} {model} {kind}").to_lowercase();
        out.push(QlcEntry { manufacturer, model, kind, path: p, key });
    }
    out.sort_by(|a, b| (a.manufacturer.to_lowercase(), a.model.to_lowercase()).cmp(&(b.manufacturer.to_lowercase(), b.model.to_lowercase())));
    out
}

struct Cap {
    min: u8,
    max: u8,
    preset: String,
    res: String,
    text: String,
}

struct ChanInfo {
    kind: K,
    default: u8,
    caps: Vec<Cap>,
    is_macro: bool,
}

fn preset_kind(p: &str) -> Option<(K, u8)> {
    let k = match p {
        "IntensityMasterDimmer" | "IntensityDimmer" => K::Dimmer,
        "IntensityRed" => K::Red,
        "IntensityGreen" => K::Green,
        "IntensityBlue" => K::Blue,
        "IntensityWhite" => K::White,
        "IntensityAmber" => K::Amber,
        "IntensityUV" => K::Uv,
        "PositionPan" => K::Pan,
        "PositionPanFine" => K::PanFine,
        "PositionTilt" => K::Tilt,
        "PositionTiltFine" => K::TiltFine,
        "SpeedPanTiltFastSlow" => K::PanTiltSpeed,
        "SpeedPanTiltSlowFast" => return Some((K::PanTiltSpeed, 255)),
        "ColorWheel" | "ColorMacro" | "ColorWheelIndex" => K::ColorWheel,
        "GoboWheel" | "GoboIndex" => K::Gobo,
        "ShutterStrobeSlowFast" => K::Strobe,
        "ShutterIrisMinToMax" => return Some((K::Other, 255)),
        "BeamFocusNearFar" | "BeamFocusFarNear" => return Some((K::Focus, 128)),
        "BeamZoomSmallBig" | "BeamZoomBigSmall" => return Some((K::Zoom, 128)),
        "PrismRotationSlowFast" | "PrismRotationFastSlow" => K::Other,
        _ => {
            if p.starts_with("Prism") {
                K::Prism
            } else {
                return None;
            }
        }
    };
    Some((k, 0))
}

fn parse_channel(node: roxmltree::Node) -> ChanInfo {
    let name = node.attribute("Name").unwrap_or("").to_lowercase();
    let caps: Vec<Cap> = node
        .children()
        .filter(|c| c.has_tag_name("Capability"))
        .map(|c| Cap {
            min: c.attribute("Min").and_then(|x| x.parse().ok()).unwrap_or(0),
            max: c.attribute("Max").and_then(|x| x.parse().ok()).unwrap_or(255),
            preset: c.attribute("Preset").unwrap_or("").to_string(),
            res: c.attribute("Res1").or(c.attribute("Res")).or(c.attribute("Color")).unwrap_or("").to_string(),
            text: c.text().unwrap_or("").to_lowercase(),
        })
        .collect();
    if let Some(p) = node.attribute("Preset") {
        if let Some((k, d)) = preset_kind(p) {
            return ChanInfo { kind: k, default: d, caps, is_macro: p == "ColorMacro" };
        }
        let k = if p.contains("Fine") { K::Other } else { K::Other };
        return ChanInfo { kind: k, default: 0, caps, is_macro: false };
    }
    let group = node.children().find(|c| c.has_tag_name("Group"));
    let gname = group.and_then(|g| g.text()).unwrap_or("").trim().to_string();
    let byte: u8 = group.and_then(|g| g.attribute("Byte")).and_then(|b| b.parse().ok()).unwrap_or(0);
    let colour = node.children().find(|c| c.has_tag_name("Colour")).and_then(|c| c.text()).unwrap_or("").to_lowercase();
    let kind = match gname.as_str() {
        "Intensity" => {
            let c = if colour.is_empty() { name.clone() } else { colour.clone() };
            if byte == 1 || name.contains("fine") {
                K::Other
            } else if c.contains("red") {
                K::Red
            } else if c.contains("green") {
                K::Green
            } else if c.contains("blue") {
                K::Blue
            } else if c.contains("white") {
                K::White
            } else if c.contains("amber") {
                K::Amber
            } else if c.contains("uv") {
                K::Uv
            } else if colour.is_empty() {
                K::Dimmer
            } else {
                K::Other
            }
        }
        "Pan" => {
            if byte == 1 {
                K::PanFine
            } else {
                K::Pan
            }
        }
        "Tilt" => {
            if byte == 1 {
                K::TiltFine
            } else {
                K::Tilt
            }
        }
        "Colour" => K::ColorWheel,
        "Gobo" => K::Gobo,
        "Prism" => K::Prism,
        "Beam" => {
            if name.contains("zoom") {
                K::Zoom
            } else if name.contains("focus") {
                K::Focus
            } else {
                K::Other
            }
        }
        "Shutter" => K::Shutter,
        "Speed" => {
            if name.contains("pan") || name.contains("tilt") || name.contains("p/t") || name.contains("movement") {
                K::PanTiltSpeed
            } else {
                K::Other
            }
        }
        _ => K::Other,
    };
    let default = match kind {
        K::Focus | K::Zoom => 128,
        K::Pan | K::Tilt => 128,
        _ => 0,
    };
    ChanInfo { kind, default, caps, is_macro: false }
}

fn off_value(caps: &[Cap]) -> u8 {
    caps.iter()
        .find(|c| c.text.contains("no function") || c.text == "off" || c.text.contains("no effect") || c.text.starts_with("off") || c.preset == "NoFunction")
        .map(|c| c.min)
        .unwrap_or(0)
}

fn parse_hex(s: &str) -> Option<[u8; 3]> {
    let s = s.trim().trim_start_matches('#');
    if s.len() < 6 {
        return None;
    }
    let v = u32::from_str_radix(&s[..6], 16).ok()?;
    Some([(v >> 16) as u8, (v >> 8) as u8, v as u8])
}

fn physical(node: roxmltree::Node, pan: &mut f32, tilt: &mut f32, beam: &mut f32) {
    for c in node.children() {
        if c.has_tag_name("Focus") {
            if let Some(p) = c.attribute("PanMax").and_then(|x| x.parse::<f32>().ok()).filter(|x| *x > 0.0) {
                *pan = p;
            }
            if let Some(t) = c.attribute("TiltMax").and_then(|x| x.parse::<f32>().ok()).filter(|x| *x > 0.0) {
                *tilt = t;
            }
        }
        if c.has_tag_name("Lens") {
            let mx = c.attribute("DegreesMax").and_then(|x| x.parse::<f32>().ok()).unwrap_or(0.0);
            let mn = c.attribute("DegreesMin").and_then(|x| x.parse::<f32>().ok()).unwrap_or(0.0);
            let d = if mx > 0.0 { mx } else { mn };
            if d > 0.0 {
                *beam = d.clamp(2.0, 70.0);
            }
        }
    }
}

/// Parse one .qxf file: one FixtureDef per mode.
pub fn load(path: &Path) -> Result<Vec<FixtureDef>, String> {
    let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    let opts = roxmltree::ParsingOptions { allow_dtd: true, ..Default::default() };
    let doc = roxmltree::Document::parse_with_options(&text, opts).map_err(|e| format!("XML non valido: {e}"))?;
    let root = doc.root_element();
    let get = |n: &str| root.children().find(|c| c.has_tag_name(n)).and_then(|c| c.text()).unwrap_or("").trim().to_string();
    let manufacturer = get("Manufacturer");
    let model = get("Model");
    let typ = get("Type");
    let mut chans: std::collections::HashMap<String, ChanInfo> = std::collections::HashMap::new();
    for c in root.children().filter(|c| c.has_tag_name("Channel")) {
        chans.insert(c.attribute("Name").unwrap_or("").to_string(), parse_channel(c));
    }
    let (mut gpan, mut gtilt, mut gbeam) = (540.0f32, 270.0f32, 25.0f32);
    if let Some(ph) = root.children().find(|c| c.has_tag_name("Physical")) {
        physical(ph, &mut gpan, &mut gtilt, &mut gbeam);
    }
    let lname = format!("{} {}", model, typ).to_lowercase();
    let mut out = vec![];
    for m in root.children().filter(|c| c.has_tag_name("Mode")) {
        let mode = m.attribute("Name").unwrap_or("Modo").to_string();
        let (mut pan, mut tilt, mut beam) = (gpan, gtilt, gbeam);
        if let Some(ph) = m.children().find(|c| c.has_tag_name("Physical")) {
            physical(ph, &mut pan, &mut tilt, &mut beam);
        }
        let mut list: Vec<(u32, String)> = m
            .children()
            .filter(|c| c.has_tag_name("Channel"))
            .map(|c| (c.attribute("Number").and_then(|x| x.parse().ok()).unwrap_or(0), c.text().unwrap_or("").trim().to_string()))
            .collect();
        list.sort_by_key(|x| x.0);
        let has_rgb = list.iter().any(|(_, n)| chans.get(n).map(|c| matches!(c.kind, K::Red | K::Green | K::Blue)).unwrap_or(false));
        let mut d = FixtureDef {
            id: format!("qlc:{manufacturer}|{model}|{mode}"),
            name: format!("{manufacturer} {model} — {mode}"),
            kind: FixtureKind::Other,
            channels: vec![],
            pan_range: pan,
            tilt_range: tilt,
            color_wheel: vec![],
            beam_angle: beam,
            shutter_open: 255,
            shutter_closed: 0,
            strobe_min: 10,
            strobe_max: 250,
            custom: true,
        };
        let mut shutter_done = false;
        for (_, n) in &list {
            let Some(ci) = chans.get(n) else {
                d.channels.push(ch(n, K::Other, 0));
                continue;
            };
            let mut kind = ci.kind;
            let mut default = ci.default;
            if kind == K::ColorWheel && (has_rgb || ci.is_macro && has_rgb) {
                // colour macros would override RGB: keep them off
                kind = K::Other;
                default = off_value(&ci.caps);
            }
            if kind == K::ColorWheel && d.color_wheel.is_empty() {
                for c in &ci.caps {
                    if c.text.contains("rot") || c.text.contains("scroll") || c.text.contains("rainbow") || c.text.contains("random") {
                        continue;
                    }
                    if let Some(col) = parse_hex(&c.res) {
                        d.color_wheel.push(ColorSlot { value: c.min, color: col });
                    } else if c.text.contains("white") || c.text.contains("open") {
                        d.color_wheel.push(ColorSlot { value: c.min, color: [255, 255, 255] });
                    }
                }
            }
            if kind == K::Shutter {
                let open = ci.caps.iter().find(|c| c.preset == "ShutterOpen" || c.text.contains("open") || c.text == "on" || c.text.starts_with("shutter on"));
                let strobe = ci.caps.iter().find(|c| c.preset.contains("Strobe") && c.preset.contains("SlowToFast") || (c.text.contains("strob") && !c.text.contains("random")));
                if open.is_none() && ci.caps.first().map(|c| c.min == 0 && (c.text.contains("no") || c.text.contains("off"))).unwrap_or(false) {
                    kind = K::Strobe;
                } else if !shutter_done {
                    shutter_done = true;
                    if let Some(o) = open {
                        d.shutter_open = ((o.min as u16 + o.max as u16) / 2) as u8;
                    }
                    if let Some(c) = ci.caps.iter().find(|c| c.preset == "ShutterClose" || c.text.contains("close") || c.text.contains("blackout")) {
                        d.shutter_closed = c.min;
                    }
                    if let Some(s) = strobe {
                        let reversed = s.preset.contains("FastToSlow") || s.text.contains("fast to slow") || s.text.contains("fast-slow");
                        (d.strobe_min, d.strobe_max) = if reversed { (s.max, s.min) } else { (s.min, s.max) };
                    }
                    default = d.shutter_open;
                }
            }
            if kind == K::Strobe {
                if let Some(s) = ci.caps.iter().find(|c| c.min > 0 && (c.text.contains("strob") || c.preset.contains("Strobe"))) {
                    d.strobe_min = s.min;
                    d.strobe_max = s.max;
                }
            }
            if kind == K::Other && default == 0 {
                default = off_value(&ci.caps);
            }
            d.channels.push(ch(n, kind, default));
        }
        let has_pan = d.has(K::Pan) || d.has(K::Tilt);
        d.kind = if lname.contains("blinder") {
            FixtureKind::Blinder
        } else if typ == "Moving Head" || typ == "Scanner" || has_pan {
            if lname.contains("beam") {
                FixtureKind::MovingBeam
            } else if lname.contains("wash") || (d.has_rgb() && !d.has(K::Gobo)) {
                FixtureKind::MovingWash
            } else {
                FixtureKind::MovingSpot
            }
        } else if typ.starts_with("LED Bar") || lname.contains(" bar") {
            FixtureKind::LedBar
        } else if typ == "Strobe" {
            FixtureKind::Strobe
        } else if typ == "Color Changer" {
            FixtureKind::Par
        } else {
            FixtureKind::Other
        };
        if !d.channels.is_empty() {
            out.push(d);
        }
    }
    if out.is_empty() {
        return Err("Nessun modo DMX nel file".into());
    }
    Ok(out)
}
