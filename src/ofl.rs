//! Fixture library from the Open Fixture Library (open-fixture-library.org).
//!
//! The fixture files are downloaded from the OFL GitHub repository into the app
//! data folder, pinned to one commit: `version.json` keeps that commit, so the
//! app can check (at most once a day) whether the library changed and update it.
//! `load` turns one OFL fixture into our `FixtureDef`, one per DMX mode.

use crate::model::ChannelKind as K;
use crate::model::*;
use serde_json::Value;
use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc, Mutex};

const REPO: &str = "OpenLightingProject/open-fixture-library";
pub const SITE: &str = "https://open-fixture-library.org";
/// Automatic check: at most once every 24 hours.
const CHECK_EVERY: u64 = 24 * 3600;
/// Below this many fixtures the downloaded archive is considered broken.
const MIN_FIXTURES: usize = 100;

#[derive(Clone, Debug)]
pub struct OflEntry {
    pub manufacturer: String,
    pub model: String,
    pub kind: String,
    pub path: PathBuf,
    /// lowercase "manufacturer model categories" for searching
    pub key: String,
}

/// The installed library version.
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct LibInfo {
    /// Commit of the OFL repository the fixtures come from.
    pub sha: String,
    /// Date of that commit (ISO 8601).
    pub date: String,
    /// Last time we asked GitHub for a newer version (unix seconds).
    pub checked: u64,
    #[serde(default)]
    pub count: usize,
}

impl LibInfo {
    pub fn short(&self) -> &str {
        &self.sha[..self.sha.len().min(7)]
    }
    pub fn day(&self) -> &str {
        &self.date[..self.date.len().min(10)]
    }
}

/// Folder of the downloaded library (inside the app data folder).
pub fn lib_dir() -> Option<PathBuf> {
    eframe::storage_dir(crate::APP_NAME).map(|d| d.join("ofl"))
}

pub fn fixtures_dir(lib: &Path) -> PathBuf {
    lib.join("fixtures")
}

pub fn read_info(lib: &Path) -> Option<LibInfo> {
    if !fixtures_dir(lib).is_dir() {
        return None;
    }
    let text = std::fs::read_to_string(lib.join("version.json")).ok()?;
    serde_json::from_str(&text).ok()
}

fn write_info(lib: &Path, info: &LibInfo) -> Result<(), String> {
    let text = serde_json::to_string_pretty(info).map_err(|e| e.to_string())?;
    std::fs::write(lib.join("version.json"), text).map_err(|e| e.to_string())
}

fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

// ------------------------------------------------------------ index

fn read_json(path: &Path) -> Option<Value> {
    serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()
}

fn manufacturer_names(fixtures: &Path) -> HashMap<String, String> {
    let mut out = HashMap::new();
    if let Some(Value::Object(m)) = read_json(&fixtures.join("manufacturers.json")) {
        for (k, v) in m {
            if let Some(n) = v.get("name").and_then(|x| x.as_str()) {
                out.insert(k, n.to_string());
            }
        }
    }
    out
}

/// Index of all fixtures of the library (name, manufacturer, categories).
pub fn scan(fixtures: &Path) -> Vec<OflEntry> {
    let names = manufacturer_names(fixtures);
    let mut out = vec![];
    let Ok(rd) = std::fs::read_dir(fixtures) else { return out };
    for man in rd.flatten() {
        let mdir = man.path();
        if !mdir.is_dir() {
            continue;
        }
        let mkey = man.file_name().to_string_lossy().to_string();
        let manufacturer = names.get(&mkey).cloned().unwrap_or_else(|| mkey.clone());
        let Ok(files) = std::fs::read_dir(&mdir) else { continue };
        for f in files.flatten() {
            let p = f.path();
            if !p.extension().map(|x| x.eq_ignore_ascii_case("json")).unwrap_or(false) {
                continue;
            }
            let Some(v) = read_json(&p) else { continue };
            if v.get("redirectTo").is_some() {
                continue;
            }
            let Some(model) = v.get("name").and_then(|x| x.as_str()).map(|s| s.to_string()) else { continue };
            let kind = v
                .get("categories")
                .and_then(|c| c.as_array())
                .map(|a| a.iter().filter_map(|x| x.as_str()).collect::<Vec<_>>().join(", "))
                .unwrap_or_default();
            let key = format!("{manufacturer} {model} {kind}").to_lowercase();
            out.push(OflEntry { manufacturer: manufacturer.clone(), model, kind, path: p, key });
        }
    }
    out.sort_by(|a, b| (a.manufacturer.to_lowercase(), a.model.to_lowercase()).cmp(&(b.manufacturer.to_lowercase(), b.model.to_lowercase())));
    out
}

// ------------------------------------------------------------ download / update

/// Latest commit of the OFL repository that changed the fixtures: (sha, date).
pub fn latest() -> Result<(String, String), String> {
    let url = format!("https://api.github.com/repos/{REPO}/commits?path=fixtures&per_page=1");
    let v = crate::update::get_json(&url)?;
    let c = v.get(0).ok_or("risposta di GitHub vuota")?;
    let sha = c.get("sha").and_then(|x| x.as_str()).filter(|s| s.len() >= 7 && s.chars().all(|c| c.is_ascii_hexdigit())).ok_or("risposta di GitHub non valida")?;
    let date = c.pointer("/commit/committer/date").and_then(|x| x.as_str()).unwrap_or("");
    Ok((sha.to_string(), date.to_string()))
}

/// Unpack `<root>/fixtures/**.json` of the repository archive into `out`.
/// Returns the number of fixtures (redirect files are skipped).
fn extract(zip_path: &Path, out: &Path) -> Result<usize, String> {
    let file = std::fs::File::open(zip_path).map_err(|e| e.to_string())?;
    let mut z = zip::ZipArchive::new(file).map_err(|e| format!("archivio non valido: {e}"))?;
    let mut n = 0;
    for i in 0..z.len() {
        let mut e = z.by_index(i).map_err(|e| format!("archivio non valido: {e}"))?;
        if !e.is_file() {
            continue;
        }
        let Some(p) = e.enclosed_name() else { continue };
        let parts: Vec<String> = p.components().map(|c| c.as_os_str().to_string_lossy().to_string()).collect();
        // "<root>/fixtures/manufacturers.json" or "<root>/fixtures/<manufacturer>/<fixture>.json"
        if parts.len() < 3 || parts.len() > 4 || parts[1] != "fixtures" || !parts.last().map(|s| s.ends_with(".json")).unwrap_or(false) {
            continue;
        }
        let mut text = String::new();
        e.read_to_string(&mut text).map_err(|e| format!("archivio non valido: {e}"))?;
        let is_fixture = parts.len() == 4;
        if is_fixture && text.contains("\"redirectTo\"") {
            continue;
        }
        let dest = parts[2..].iter().fold(out.to_path_buf(), |p, c| p.join(c));
        if let Some(d) = dest.parent() {
            std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
        }
        std::fs::write(&dest, text).map_err(|e| e.to_string())?;
        if is_fixture {
            n += 1;
        }
    }
    Ok(n)
}

/// Download the fixtures at commit `sha` and replace the installed library.
fn install(sha: &str, date: &str, progress: &Arc<Mutex<f32>>) -> Result<LibInfo, String> {
    let lib = lib_dir().ok_or("cartella dati dell'app non disponibile")?;
    let parent = lib.parent().ok_or("cartella dati dell'app non disponibile")?.to_path_buf();
    std::fs::create_dir_all(&parent).map_err(|e| e.to_string())?;
    let zip_path = std::env::temp_dir().join("LightScio-ofl.zip");
    let url = format!("https://codeload.github.com/{REPO}/zip/{sha}");
    crate::update::download_file(&url, &zip_path, 4_500_000, progress)?;
    let tmp = parent.join("ofl.new");
    let _ = std::fs::remove_dir_all(&tmp);
    let res = extract(&zip_path, &fixtures_dir(&tmp));
    let _ = std::fs::remove_file(&zip_path);
    let n = match res {
        Ok(n) if n >= MIN_FIXTURES => n,
        Ok(n) => {
            let _ = std::fs::remove_dir_all(&tmp);
            return Err(format!("archivio incompleto ({n} fixture)"));
        }
        Err(e) => {
            let _ = std::fs::remove_dir_all(&tmp);
            return Err(e);
        }
    };
    let info = LibInfo { sha: sha.to_string(), date: date.to_string(), checked: now(), count: n };
    write_info(&tmp, &info)?;
    // swap: the old library stays usable until the new one is complete
    let old = parent.join("ofl.old");
    let _ = std::fs::remove_dir_all(&old);
    if lib.exists() {
        std::fs::rename(&lib, &old).map_err(|e| format!("impossibile sostituire la libreria: {e}"))?;
    }
    if let Err(e) = std::fs::rename(&tmp, &lib) {
        let _ = std::fs::rename(&old, &lib);
        return Err(format!("impossibile sostituire la libreria: {e}"));
    }
    let _ = std::fs::remove_dir_all(&old);
    Ok(info)
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Update {
    /// Only read the installed library.
    Never,
    /// Check for a newer version if the last check is older than a day (or nothing is installed).
    IfDue,
    /// Check now.
    Now,
}

pub enum Msg {
    /// What the background job is doing (Italian source text, translated by the UI).
    Busy(&'static str),
    /// Installed library read from disk.
    Loaded(Vec<OflEntry>, LibInfo),
    /// A newer library was downloaded and installed.
    Updated(Vec<OflEntry>, LibInfo),
    /// Checked: already the latest version.
    UpToDate(LibInfo),
    Failed(String),
    Done,
}

/// Read the installed library and, depending on `mode`, check / download a newer one.
pub fn spawn(mode: Update, progress: Arc<Mutex<f32>>) -> mpsc::Receiver<Msg> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        run(mode, &progress, &tx);
        let _ = tx.send(Msg::Done);
    });
    rx
}

fn run(mode: Update, progress: &Arc<Mutex<f32>>, tx: &mpsc::Sender<Msg>) {
    let Some(lib) = lib_dir() else {
        let _ = tx.send(Msg::Failed("cartella dati dell'app non disponibile".into()));
        return;
    };
    let info = read_info(&lib);
    if let Some(i) = &info {
        let _ = tx.send(Msg::Busy("Leggo la libreria delle fixture…"));
        let _ = tx.send(Msg::Loaded(scan(&fixtures_dir(&lib)), i.clone()));
    }
    let due = match mode {
        Update::Never => false,
        Update::Now => true,
        Update::IfDue => info.as_ref().map(|i| now().saturating_sub(i.checked) > CHECK_EVERY).unwrap_or(true),
    };
    if !due {
        return;
    }
    let _ = tx.send(Msg::Busy("Controllo se la libreria è aggiornata…"));
    let (sha, date) = match latest() {
        Ok(x) => x,
        Err(e) => {
            let _ = tx.send(Msg::Failed(e));
            return;
        }
    };
    if let Some(mut i) = info.filter(|i| i.sha == sha) {
        i.checked = now();
        let _ = write_info(&lib, &i);
        let _ = tx.send(Msg::UpToDate(i));
        return;
    }
    if let Ok(mut p) = progress.lock() {
        *p = 0.0;
    }
    let _ = tx.send(Msg::Busy("Scarico la libreria delle fixture…"));
    match install(&sha, &date, progress) {
        Ok(i) => {
            let _ = tx.send(Msg::Updated(scan(&fixtures_dir(&lib)), i));
        }
        Err(e) => {
            let _ = tx.send(Msg::Failed(e));
        }
    }
}

// ------------------------------------------------------------ conversion

fn s<'a>(v: &'a Value, k: &str) -> &'a str {
    v.get(k).and_then(|x| x.as_str()).unwrap_or("")
}

/// "540deg" -> 540
fn degrees(v: &Value) -> Option<f32> {
    v.as_str()?.trim().strip_suffix("deg")?.trim().parse().ok()
}

/// Speed as a comparable number (higher = faster).
fn speed(v: &Value) -> Option<f64> {
    let t = v.as_str()?.trim().to_lowercase();
    let num = |suffix: &str| t.strip_suffix(suffix).and_then(|x| x.trim().parse::<f64>().ok());
    match t.as_str() {
        "fast" => Some(1e6),
        "slow" => Some(1e-6),
        "stop" => Some(0.0),
        _ => num("hz").or_else(|| num("bpm").map(|x| x / 60.0)).or_else(|| num("rpm").map(|x| x / 60.0)).or_else(|| num("%")),
    }
}

/// Duration as a comparable number in seconds (lower = faster).
fn duration(v: &Value) -> Option<f64> {
    let t = v.as_str()?.trim().to_lowercase();
    match t.as_str() {
        "instant" => Some(0.0),
        "short" => Some(1.0),
        "long" => Some(1e6),
        _ => t
            .strip_suffix("ms")
            .and_then(|x| x.trim().parse::<f64>().ok().map(|x| x / 1000.0))
            .or_else(|| t.strip_suffix('s').and_then(|x| x.trim().parse::<f64>().ok()))
            .or_else(|| t.strip_suffix('%').and_then(|x| x.trim().parse::<f64>().ok())),
    }
}

/// Does the capability go from fast (start of the range) to slow (end)?
fn fast_to_slow(c: &Value) -> Option<bool> {
    if let (Some(a), Some(b)) = (c.get("speedStart").and_then(speed), c.get("speedEnd").and_then(speed)) {
        return Some(a > b);
    }
    if let (Some(a), Some(b)) = (c.get("durationStart").and_then(duration), c.get("durationEnd").and_then(duration)) {
        return Some(a < b);
    }
    None
}

fn parse_hex(s: &str) -> Option<[u8; 3]> {
    let s = s.trim().trim_start_matches('#');
    if s.len() < 6 {
        return None;
    }
    let v = u32::from_str_radix(&s[..6], 16).ok()?;
    Some([(v >> 16) as u8, (v >> 8) as u8, v as u8])
}

struct Cap<'a> {
    start: u8,
    end: u8,
    typ: &'a str,
    v: &'a Value,
}

impl Cap<'_> {
    fn mid(&self) -> u8 {
        ((self.start as u16 + self.end as u16) / 2) as u8
    }
}

/// Number of bytes of the DMX values written in the channel ("8bit" = 1).
fn resolution(c: &Value) -> u32 {
    match s(c, "dmxValueResolution") {
        "8bit" => 1,
        "16bit" => 2,
        "24bit" => 3,
        _ => c.get("fineChannelAliases").and_then(|a| a.as_array()).map(|a| a.len() as u32 + 1).unwrap_or(1),
    }
}

fn to_8bit(x: f64, res: u32) -> u8 {
    (x / 256f64.powi(res as i32 - 1)).round().clamp(0.0, 255.0) as u8
}

fn caps(c: &Value) -> Vec<Cap<'_>> {
    let res = resolution(c);
    if let Some(cap) = c.get("capability") {
        return vec![Cap { start: 0, end: 255, typ: s(cap, "type"), v: cap }];
    }
    c.get("capabilities")
        .and_then(|x| x.as_array())
        .map(|a| {
            a.iter()
                .map(|cap| {
                    let r = cap.get("dmxRange").and_then(|r| r.as_array());
                    let at = |i: usize, d: f64| r.and_then(|r| r.get(i)).and_then(|x| x.as_f64()).unwrap_or(d);
                    Cap { start: to_8bit(at(0, 0.0), res), end: to_8bit(at(1, 255.0 * 256f64.powi(res as i32 - 1)), res), typ: s(cap, "type"), v: cap }
                })
                .collect()
        })
        .unwrap_or_default()
}

fn default_value(c: &Value) -> Option<u8> {
    match c.get("defaultValue")? {
        Value::Number(n) => Some(to_8bit(n.as_f64()?, resolution(c))),
        Value::String(t) => t.trim().strip_suffix('%')?.trim().parse::<f64>().ok().map(|p| (p * 2.55).round().clamp(0.0, 255.0) as u8),
        _ => None,
    }
}

/// The capability type that covers most of the DMX range (ignoring "no function").
fn main_type<'a>(cs: &[Cap<'a>]) -> &'a str {
    let mut span: Vec<(&str, u32)> = vec![];
    for c in cs.iter().filter(|c| c.typ != "NoFunction") {
        let w = c.end.saturating_sub(c.start) as u32 + 1;
        match span.iter_mut().find(|x| x.0 == c.typ) {
            Some(x) => x.1 += w,
            None => span.push((c.typ, w)),
        }
    }
    span.iter().fold(("", 0), |best, x| if x.1 > best.1 { *x } else { best }).0
}

/// DMX value that leaves the channel without effect.
fn off_value(cs: &[Cap]) -> u8 {
    cs.iter().find(|c| c.typ == "NoFunction").map(|c| c.start).unwrap_or(0)
}

fn color_kind(c: &str) -> K {
    match c {
        "Red" => K::Red,
        "Green" => K::Green,
        "Blue" => K::Blue,
        "White" | "Warm White" | "Cold White" => K::White,
        "Amber" => K::Amber,
        "UV" => K::Uv,
        _ => K::Other,
    }
}

/// Keys of the matrix pixels in the order asked by `repeatFor`.
fn pixel_keys(matrix: &Value, order: &str) -> Vec<String> {
    // (x, y, z, key)
    let mut px: Vec<(usize, usize, usize, String)> = vec![];
    if let Some(zs) = matrix.get("pixelKeys").and_then(|x| x.as_array()) {
        for (z, ys) in zs.iter().enumerate() {
            for (y, xs) in ys.as_array().into_iter().flatten().enumerate() {
                for (x, k) in xs.as_array().into_iter().flatten().enumerate() {
                    if let Some(k) = k.as_str() {
                        px.push((x, y, z, k.to_string()));
                    }
                }
            }
        }
    } else if let Some(n) = matrix.get("pixelCount").and_then(|x| x.as_array()) {
        let dim = |i: usize| n.get(i).and_then(|x| x.as_u64()).unwrap_or(1).max(1) as usize;
        let (nx, ny, nz) = (dim(0), dim(1), dim(2));
        let used = [nx, ny, nz].iter().filter(|d| **d > 1).count();
        for z in 0..nz {
            for y in 0..ny {
                for x in 0..nx {
                    let key = if used <= 1 {
                        (x.max(y).max(z) + 1).to_string()
                    } else if nz == 1 {
                        format!("({}, {})", x + 1, y + 1)
                    } else {
                        format!("({}, {}, {})", x + 1, y + 1, z + 1)
                    };
                    px.push((x, y, z, key));
                }
            }
        }
    }
    let letters: Vec<char> = order.trim_start_matches("eachPixel").chars().collect();
    if letters.len() == 3 && letters.iter().all(|c| matches!(c, 'X' | 'Y' | 'Z')) {
        // the first letter changes fastest
        let coord = |p: &(usize, usize, usize, String), c: char| match c {
            'X' => p.0,
            'Y' => p.1,
            _ => p.2,
        };
        px.sort_by_key(|p| (coord(p, letters[2]), coord(p, letters[1]), coord(p, letters[0])));
    } else {
        px.sort_by(|a, b| natural_cmp(&a.3, &b.3));
    }
    px.into_iter().map(|p| p.3).collect()
}

fn natural_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    let split = |s: &str| -> Vec<(bool, String)> {
        let mut out: Vec<(bool, String)> = vec![];
        for ch in s.chars() {
            let d = ch.is_ascii_digit();
            match out.last_mut() {
                Some((pd, t)) if *pd == d => t.push(ch),
                _ => out.push((d, ch.to_string())),
            }
        }
        out
    };
    let (x, y) = (split(a), split(b));
    for (p, q) in x.iter().zip(y.iter()) {
        let o = if p.0 && q.0 {
            p.1.parse::<u64>().unwrap_or(0).cmp(&q.1.parse::<u64>().unwrap_or(0))
        } else {
            p.1.to_lowercase().cmp(&q.1.to_lowercase())
        };
        if o != std::cmp::Ordering::Equal {
            return o;
        }
    }
    x.len().cmp(&y.len())
}

/// One channel as found in the fixture: the JSON definition, the name of the
/// definition (used as default wheel name) and which byte it is (0 = coarse).
#[derive(Clone)]
struct Src<'a> {
    def: &'a Value,
    base: String,
    fine: u32,
}

fn channel_table<'a>(fx: &'a Value) -> (HashMap<String, Src<'a>>, HashMap<String, String>) {
    let mut table: HashMap<String, Src<'a>> = HashMap::new();
    let mut switches: HashMap<String, String> = HashMap::new();
    let add = |name: String, def: &'a Value, key: &str, table: &mut HashMap<String, Src<'a>>, switches: &mut HashMap<String, String>| {
        let rep = |t: &str| t.replace("$pixelKey", key);
        for (i, a) in def.get("fineChannelAliases").and_then(|a| a.as_array()).into_iter().flatten().enumerate() {
            if let Some(a) = a.as_str() {
                table.insert(rep(a), Src { def, base: name.clone(), fine: i as u32 + 1 });
            }
        }
        // switching channels: the alias points to the channel active at the first capability
        for cap in def.get("capabilities").and_then(|a| a.as_array()).into_iter().flatten() {
            if let Some(Value::Object(m)) = cap.get("switchChannels") {
                for (alias, target) in m {
                    if let Some(t) = target.as_str() {
                        switches.entry(rep(alias)).or_insert_with(|| rep(t));
                    }
                }
            }
        }
        table.insert(name.clone(), Src { def, base: name, fine: 0 });
    };
    if let Some(Value::Object(m)) = fx.get("availableChannels") {
        for (n, d) in m {
            add(n.clone(), d, "", &mut table, &mut switches);
        }
    }
    if let Some(Value::Object(m)) = fx.get("templateChannels") {
        let matrix = fx.get("matrix").cloned().unwrap_or(Value::Null);
        let mut keys = pixel_keys(&matrix, "eachPixelABC");
        if let Some(Value::Object(g)) = matrix.get("pixelGroups") {
            keys.extend(g.keys().cloned());
        }
        for (n, d) in m {
            for k in &keys {
                add(n.replace("$pixelKey", k), d, k, &mut table, &mut switches);
            }
        }
    }
    (table, switches)
}

/// Channel names of a mode, with the matrix inserts expanded (None = unused channel).
fn mode_channels(fx: &Value, mode: &Value) -> Vec<Option<String>> {
    let matrix = fx.get("matrix").cloned().unwrap_or(Value::Null);
    let mut out = vec![];
    for c in mode.get("channels").and_then(|a| a.as_array()).into_iter().flatten() {
        match c {
            Value::String(n) => out.push(Some(n.clone())),
            Value::Object(m) if m.get("insert").and_then(|x| x.as_str()) == Some("matrixChannels") => {
                let keys: Vec<String> = match m.get("repeatFor") {
                    Some(Value::Array(a)) => a.iter().filter_map(|x| x.as_str().map(|s| s.to_string())).collect(),
                    Some(Value::String(r)) if r == "eachPixelGroup" => {
                        matrix.get("pixelGroups").and_then(|g| g.as_object()).map(|g| g.keys().cloned().collect()).unwrap_or_default()
                    }
                    Some(Value::String(r)) => pixel_keys(&matrix, r),
                    _ => vec![],
                };
                let templ: Vec<&str> = m.get("templateChannels").and_then(|a| a.as_array()).into_iter().flatten().filter_map(|x| x.as_str()).collect();
                if m.get("channelOrder").and_then(|x| x.as_str()) == Some("perChannel") {
                    for t in &templ {
                        for k in &keys {
                            out.push(Some(t.replace("$pixelKey", k)));
                        }
                    }
                } else {
                    for k in &keys {
                        for t in &templ {
                            out.push(Some(t.replace("$pixelKey", k)));
                        }
                    }
                }
            }
            _ => out.push(None),
        }
    }
    out
}

/// Pan / tilt range and beam angle from a "physical" block.
fn physical(p: Option<&Value>, pan: &mut f32, tilt: &mut f32, beam: &mut f32) {
    let Some(p) = p else { return };
    if let Some(v) = p.pointer("/focus/panMax").and_then(|x| x.as_f64()).filter(|x| *x > 0.0) {
        *pan = v as f32;
    }
    if let Some(v) = p.pointer("/focus/tiltMax").and_then(|x| x.as_f64()).filter(|x| *x > 0.0) {
        *tilt = v as f32;
    }
    if let Some(r) = p.pointer("/lens/degreesMinMax").and_then(|x| x.as_array()) {
        let mn = r.first().and_then(|x| x.as_f64()).unwrap_or(0.0) as f32;
        let mx = r.get(1).and_then(|x| x.as_f64()).unwrap_or(0.0) as f32;
        let d = if mx > 0.0 { mx } else { mn };
        if d > 0.0 {
            *beam = d.clamp(2.0, 70.0);
        }
    }
}

/// Kind of a wheel from its slots.
fn wheel_kind(fx: &Value, wheel: &str) -> K {
    let slots = fx.pointer(&format!("/wheels/{}", wheel.replace('~', "~0").replace('/', "~1"))).and_then(|w| w.get("slots")).and_then(|s| s.as_array());
    let has = |t: &str| slots.map(|a| a.iter().any(|x| s(x, "type") == t)).unwrap_or(false);
    if has("Color") {
        K::ColorWheel
    } else if has("Gobo") || has("AnimationGoboStart") {
        K::Gobo
    } else if has("Prism") {
        K::Prism
    } else {
        K::Other
    }
}

fn wheel_slot<'a>(fx: &'a Value, wheel: &str, n: usize) -> Option<&'a Value> {
    fx.pointer(&format!("/wheels/{}", wheel.replace('~', "~0").replace('/', "~1")))?.get("slots")?.as_array()?.get(n.checked_sub(1)?)
}

fn cap_wheel<'a>(c: &'a Cap, base: &'a str) -> &'a str {
    match c.v.get("wheel") {
        Some(Value::String(w)) => w,
        _ => base,
    }
}

// ------------------------------------------------------------ data sheet / capabilities

fn hex_list(v: Option<&Value>) -> Vec<[u8; 3]> {
    v.and_then(|x| x.as_array()).into_iter().flatten().filter_map(|x| x.as_str()).filter_map(parse_hex).collect()
}

/// A value as written by OFL, a bit friendlier ("540deg" -> "540°").
fn pretty(v: &Value) -> String {
    match v {
        Value::String(t) => match t.strip_suffix("deg") {
            Some(n) => format!("{n}°"),
            None => t.clone(),
        },
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        _ => String::new(),
    }
}

/// Properties of a capability that hold values (or "<name>Start" / "<name>End" ranges).
const VALUE_KEYS: [&str; 17] = [
    "speed",
    "duration",
    "time",
    "angle",
    "brightness",
    "colorTemperature",
    "distance",
    "openPercent",
    "frostIntensity",
    "parameter",
    "fogOutput",
    "soundSensitivity",
    "horizontalAngle",
    "verticalAngle",
    "insertion",
    "shakeAngle",
    "shakeSpeed",
];

fn cap_values(c: &Value) -> String {
    let mut out = vec![];
    for k in VALUE_KEYS {
        if let Some(v) = c.get(k) {
            out.push(pretty(v));
        } else if let (Some(a), Some(b)) = (c.get(format!("{k}Start").as_str()), c.get(format!("{k}End").as_str())) {
            let (a, b) = (pretty(a), pretty(b));
            out.push(if a == b { a } else { format!("{a} → {b}") });
        }
    }
    out.retain(|x| !x.is_empty());
    out.join(", ")
}

fn slot_label(slot: &Value) -> String {
    if let Some(n) = slot.get("name").and_then(|x| x.as_str()) {
        return n.to_string();
    }
    let typ = s(slot, "type");
    match (typ, slot.get("resource")) {
        ("Gobo", Some(Value::String(r))) => r.rsplit('/').next().unwrap_or(r).to_string(),
        ("Prism", _) => match slot.get("facets").and_then(|x| x.as_u64()) {
            Some(f) => format!("Prism {f}"),
            None => "Prism".into(),
        },
        _ => typ.to_string(),
    }
}

/// Name and colours of the wheel slot number `n` (a fraction = between two slots).
fn slot_at(fx: &Value, wheel: &str, n: f64) -> (String, Vec<[u8; 3]>) {
    let one = |k: usize| wheel_slot(fx, wheel, k).map(|sl| (slot_label(sl), hex_list(sl.get("colors")))).unwrap_or_else(|| (format!("{k}"), vec![]));
    if n.fract() == 0.0 {
        one(n as usize)
    } else {
        let (a, ca) = one(n.floor() as usize);
        let (b, cb) = one(n.ceil() as usize);
        (format!("{a} / {b}"), ca.into_iter().chain(cb).collect())
    }
}

fn cap_info(fx: &Value, c: &Cap, base: &str) -> CapInfo {
    let v = c.v;
    let mut colors = hex_list(v.get("colors"));
    let what = match c.typ {
        "ShutterStrobe" => {
            let mut w = s(v, "shutterEffect").to_string();
            if v.get("randomTiming").and_then(|x| x.as_bool()) == Some(true) {
                w.push_str(" (random)");
            }
            if v.get("soundControlled").and_then(|x| x.as_bool()) == Some(true) {
                w.push_str(" (sound)");
            }
            w
        }
        "ColorIntensity" => s(v, "color").to_string(),
        "WheelSlot" | "WheelShake" | "WheelSlotRotation" | "WheelRotation" => {
            let wheel = cap_wheel(c, base).to_string();
            let num = |k: &str| v.get(k).and_then(|x| x.as_f64());
            let slot = if let Some(n) = num("slotNumber") {
                let (name, cols) = slot_at(fx, &wheel, n);
                colors.extend(cols);
                Some(name)
            } else if let (Some(a), Some(b)) = (num("slotNumberStart"), num("slotNumberEnd")) {
                Some(format!("{} → {}", slot_at(fx, &wheel, a).0, slot_at(fx, &wheel, b).0))
            } else {
                None
            };
            match slot {
                Some(sl) => format!("{wheel}: {sl}"),
                None => wheel,
            }
        }
        "Effect" => v.get("effectName").or(v.get("effectPreset")).map(pretty).unwrap_or_default(),
        "BladeInsertion" | "BladeRotation" => v.get("blade").map(pretty).unwrap_or_default(),
        "Fog" | "FogType" => s(v, "fogType").to_string(),
        _ => String::new(),
    };
    let mut values = cap_values(v);
    if let Some(h) = v.get("hold").map(pretty).filter(|h| !h.is_empty()) {
        values = if values.is_empty() { format!("hold {h}") } else { format!("{values}, hold {h}") };
    }
    CapInfo { start: c.start, end: c.end, typ: c.typ.to_string(), what, values, comment: s(v, "comment").to_string(), colors }
}

fn wheels(fx: &Value) -> Vec<WheelInfo> {
    let Some(Value::Object(m)) = fx.get("wheels") else { return vec![] };
    m.iter()
        .map(|(name, w)| WheelInfo {
            name: name.clone(),
            slots: w
                .get("slots")
                .and_then(|x| x.as_array())
                .into_iter()
                .flatten()
                .map(|sl| {
                    let mut detail = vec![];
                    if let Some(t) = sl.get("colorTemperature").map(pretty) {
                        detail.push(t);
                    }
                    if let Some(f) = sl.get("facets").and_then(|x| x.as_u64()) {
                        detail.push(format!("{f} facets"));
                    }
                    if let Some(Value::String(r)) = sl.get("resource") {
                        detail.push(r.clone());
                    }
                    SlotInfo { typ: s(sl, "type").to_string(), name: s(sl, "name").to_string(), colors: hex_list(sl.get("colors")), detail: detail.join(", ") }
                })
                .collect(),
        })
        .collect()
}

/// Data sheet for one mode (the mode's physical data override the fixture's).
fn fixture_info(fx: &Value, mode: &Value, man_key: &str, fixture_key: &str, manufacturer: &str, version: &str) -> FixtureInfo {
    let ph = |path: &str| mode.get("physical").and_then(|p| p.pointer(path)).or_else(|| fx.get("physical").and_then(|p| p.pointer(path)));
    let f = |path: &str| ph(path).and_then(|x| x.as_f64()).map(|x| x as f32);
    let txt = |path: &str| ph(path).and_then(|x| x.as_str()).unwrap_or("").to_string();
    let arr = |path: &str| -> Vec<f32> { ph(path).and_then(|x| x.as_array()).into_iter().flatten().filter_map(|x| x.as_f64()).map(|x| x as f32).collect() };
    let angle = |path: &str| match ph(path) {
        Some(Value::Number(n)) => format!("{n}°"),
        Some(Value::String(t)) if t == "infinite" => "∞".into(),
        Some(v) => pretty(v),
        None => String::new(),
    };
    let dims = arr("/dimensions");
    let lens = arr("/lens/degreesMinMax");
    let strs = |v: Option<&Value>| -> Vec<String> { v.and_then(|x| x.as_array()).into_iter().flatten().filter_map(|x| x.as_str().map(|s| s.to_string())).collect() };
    let matrix = fx.get("matrix").and_then(|m| {
        if let Some(n) = m.get("pixelCount").and_then(|x| x.as_array()) {
            let d = |i: usize| n.get(i).and_then(|x| x.as_u64()).unwrap_or(1) as u32;
            return Some([d(0), d(1), d(2)]);
        }
        let zs = m.get("pixelKeys")?.as_array()?;
        let ny = zs.iter().filter_map(|y| y.as_array()).map(|y| y.len()).max().unwrap_or(0);
        let nx = zs.iter().filter_map(|y| y.as_array()).flatten().filter_map(|x| x.as_array()).map(|x| x.len()).max().unwrap_or(0);
        Some([nx as u32, ny as u32, zs.len() as u32])
    });
    let mut links = vec![];
    if let Some(Value::Object(l)) = fx.get("links") {
        for (kind, urls) in l {
            for u in strs(Some(urls)) {
                links.push((kind.clone(), u));
            }
        }
    }
    FixtureInfo {
        library: "Open Fixture Library".into(),
        key: format!("{man_key}/{fixture_key}"),
        version: version.to_string(),
        manufacturer: manufacturer.to_string(),
        model: s(fx, "name").to_string(),
        short_name: s(fx, "shortName").to_string(),
        categories: strs(fx.get("categories")),
        mode: s(mode, "name").to_string(),
        mode_short: s(mode, "shortName").to_string(),
        comment: s(fx, "comment").to_string(),
        dimensions: (dims.len() == 3).then(|| [dims[0], dims[1], dims[2]]),
        weight: f("/weight"),
        power: f("/power"),
        dmx_connector: txt("/DMXconnector"),
        bulb: txt("/bulb/type"),
        color_temperature: f("/bulb/colorTemperature"),
        lumens: f("/bulb/lumens"),
        lens: txt("/lens/name"),
        lens_degrees: (lens.len() == 2).then(|| [lens[0], lens[1]]),
        focus_type: txt("/focus/type"),
        pan_max: angle("/focus/panMax"),
        tilt_max: angle("/focus/tiltMax"),
        matrix,
        links,
        authors: strs(fx.pointer("/meta/authors")),
        created: fx.pointer("/meta/createDate").and_then(|x| x.as_str()).unwrap_or("").to_string(),
        modified: fx.pointer("/meta/lastModifyDate").and_then(|x| x.as_str()).unwrap_or("").to_string(),
    }
}

/// Convert one OFL fixture (already parsed): one FixtureDef per mode.
/// Also returns the channel names that could not be resolved.
fn convert(fx: &Value, man_key: &str, manufacturer: &str, fixture_key: &str, version: &str) -> (Vec<FixtureDef>, Vec<String>) {
    let model = s(fx, "name").to_string();
    let cats: Vec<&str> = fx.get("categories").and_then(|a| a.as_array()).into_iter().flatten().filter_map(|x| x.as_str()).collect();
    let (table, switches) = channel_table(fx);
    let resolve = |name: &str| -> Option<Src> {
        table.get(name).cloned().or_else(|| switches.get(name).and_then(|t| table.get(t)).cloned())
    };
    let (mut gpan, mut gtilt, mut gbeam) = (540.0f32, 270.0f32, 25.0f32);
    physical(fx.get("physical"), &mut gpan, &mut gtilt, &mut gbeam);
    let lname = model.to_lowercase();
    let mut out = vec![];
    let mut unresolved = vec![];
    for m in fx.get("modes").and_then(|a| a.as_array()).into_iter().flatten() {
        let mode = s(m, "name").to_string();
        let (mut pan, mut tilt, mut beam) = (gpan, gtilt, gbeam);
        physical(m.get("physical"), &mut pan, &mut tilt, &mut beam);
        let names = mode_channels(fx, m);
        let srcs: Vec<Option<Src>> = names.iter().map(|n| n.as_deref().and_then(resolve)).collect();
        let kind_of = |src: &Src| -> K {
            let cs = caps(src.def);
            match main_type(&cs) {
                "Intensity" => K::Dimmer,
                "ColorIntensity" => color_kind(cs.iter().find(|c| c.typ == "ColorIntensity").map(|c| s(c.v, "color")).unwrap_or("")),
                _ => K::Other,
            }
        };
        let has_rgb = srcs.iter().flatten().any(|x| x.fine == 0 && matches!(kind_of(x), K::Red | K::Green | K::Blue));
        let mut d = FixtureDef {
            id: format!("ofl:{man_key}/{fixture_key}|{mode}"),
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
            wheels: wheels(fx),
            info: Some(Box::new(fixture_info(fx, m, man_key, fixture_key, manufacturer, version))),
        };
        let mut shutter_done = false;
        // kind of each coarse channel, so a fine channel knows what it refines
        let mut coarse: HashMap<String, K> = HashMap::new();
        for (i, name) in names.iter().enumerate() {
            let label = name.clone().unwrap_or_else(|| "—".into());
            let Some(src) = &srcs[i] else {
                if name.is_some() {
                    unresolved.push(label.clone());
                }
                d.channels.push(ch(&label, K::Other, 0));
                continue;
            };
            let cs = caps(src.def);
            let infos = if src.fine > 0 {
                vec![CapInfo { start: 0, end: 255, typ: "Fine".into(), what: src.base.clone(), ..Default::default() }]
            } else {
                cs.iter().map(|c| cap_info(fx, c, &src.base)).collect()
            };
            if src.fine > 0 {
                let parent = coarse.get(&src.base).copied().unwrap_or(K::Other);
                let k = match (parent, src.fine) {
                    (K::Pan, 1) => K::PanFine,
                    (K::Tilt, 1) => K::TiltFine,
                    _ => K::Other,
                };
                d.channels.push(ChannelDef { caps: infos, ..ch(&label, k, 0) });
                continue;
            }
            let explicit = default_value(src.def);
            let mut kind;
            let mut default = 0u8;
            match main_type(&cs) {
                "Intensity" => kind = K::Dimmer,
                "ColorIntensity" => kind = kind_of(src),
                "Pan" | "PanContinuous" => {
                    kind = K::Pan;
                    default = 128;
                    if let Some(c) = cs.iter().find(|c| c.typ == "Pan") {
                        if let (Some(a), Some(b)) = (c.v.get("angleStart").and_then(degrees), c.v.get("angleEnd").and_then(degrees)) {
                            if (a - b).abs() > 0.0 {
                                d.pan_range = (a - b).abs();
                            }
                        }
                    }
                }
                "Tilt" | "TiltContinuous" => {
                    kind = K::Tilt;
                    default = 128;
                    if let Some(c) = cs.iter().find(|c| c.typ == "Tilt") {
                        if let (Some(a), Some(b)) = (c.v.get("angleStart").and_then(degrees), c.v.get("angleEnd").and_then(degrees)) {
                            if (a - b).abs() > 0.0 {
                                d.tilt_range = (a - b).abs();
                            }
                        }
                    }
                }
                "PanTiltSpeed" => {
                    kind = K::PanTiltSpeed;
                    // fastest movement by default
                    if let Some(c) = cs.iter().find(|c| c.typ == "PanTiltSpeed") {
                        default = match fast_to_slow(c.v) {
                            Some(true) => c.start,
                            Some(false) => c.end,
                            None => 0,
                        };
                    }
                }
                "ShutterStrobe" | "StrobeSpeed" => {
                    let effect = |c: &&Cap, e: &str| c.typ == "ShutterStrobe" && s(c.v, "shutterEffect") == e;
                    let open = cs.iter().find(|c| effect(c, "Open"));
                    let closed = cs.iter().find(|c| effect(c, "Closed"));
                    let strobe = cs
                        .iter()
                        .find(|c| effect(c, "Strobe") && c.v.get("randomTiming").and_then(|x| x.as_bool()) != Some(true) && c.end > c.start)
                        .or_else(|| cs.iter().find(|c| effect(c, "Strobe")));
                    let range = |c: &Cap| if fast_to_slow(c.v) == Some(true) { (c.end, c.start) } else { (c.start, c.end) };
                    if open.is_none() && closed.is_none() {
                        kind = K::Strobe;
                        default = off_value(&cs);
                        if let Some(c) = strobe {
                            (d.strobe_min, d.strobe_max) = range(c);
                        }
                    } else if !shutter_done {
                        shutter_done = true;
                        kind = K::Shutter;
                        if let Some(o) = open {
                            d.shutter_open = o.mid();
                        }
                        if let Some(c) = closed {
                            d.shutter_closed = c.start;
                        }
                        if let Some(c) = strobe {
                            (d.strobe_min, d.strobe_max) = range(c);
                        }
                        default = d.shutter_open;
                    } else {
                        kind = K::Other;
                        default = open.map(|o| o.mid()).unwrap_or_else(|| off_value(&cs));
                    }
                }
                "WheelSlot" | "WheelRotation" | "WheelShake" | "WheelSlotRotation" => {
                    let slot = cs.iter().find(|c| c.typ == "WheelSlot");
                    kind = slot.map(|c| wheel_kind(fx, cap_wheel(c, &src.base))).unwrap_or(K::Other);
                }
                "ColorPreset" => kind = K::ColorWheel,
                "Prism" => kind = K::Prism,
                "Focus" => {
                    kind = K::Focus;
                    default = 128;
                }
                "Zoom" => {
                    kind = K::Zoom;
                    default = 128;
                }
                "Iris" => {
                    kind = K::Other;
                    let open = |v: Option<&Value>| matches!(v.and_then(|x| x.as_str()), Some("open") | Some("100%"));
                    if let Some(c) = cs.iter().find(|c| c.typ == "Iris") {
                        default = if open(c.v.get("openPercentStart").or(c.v.get("openPercent"))) {
                            c.start
                        } else if open(c.v.get("openPercentEnd")) {
                            c.end
                        } else {
                            0
                        };
                    }
                }
                _ => {
                    kind = K::Other;
                    default = off_value(&cs);
                }
            }
            if kind == K::ColorWheel && has_rgb {
                // colour macros / wheels would override RGB: keep them off
                kind = K::Other;
                default = off_value(&cs);
            }
            if kind == K::ColorWheel && d.color_wheel.is_empty() {
                for c in &cs {
                    let color = match c.typ {
                        "WheelSlot" => {
                            let n = c.v.get("slotNumber").and_then(|x| x.as_f64()).filter(|x| x.fract() == 0.0);
                            n.and_then(|n| wheel_slot(fx, cap_wheel(c, &src.base), n as usize)).and_then(|slot| match s(slot, "type") {
                                "Color" => slot.pointer("/colors/0").and_then(|x| x.as_str()).and_then(parse_hex),
                                "Open" => Some([255, 255, 255]),
                                _ => None,
                            })
                        }
                        "ColorPreset" => c.v.pointer("/colors/0").and_then(|x| x.as_str()).and_then(parse_hex),
                        _ => None,
                    };
                    if let Some(col) = color {
                        d.color_wheel.push(ColorSlot { value: c.mid(), color: col });
                    }
                }
            }
            // an explicit default wins, except where we need a working value
            if let Some(v) = explicit {
                if !matches!(kind, K::Shutter | K::PanTiltSpeed) {
                    default = v;
                }
            }
            coarse.insert(src.base.clone(), kind);
            d.channels.push(ChannelDef { caps: infos, ..ch(&label, kind, default) });
        }
        let has_pan = d.has(K::Pan) || d.has(K::Tilt);
        let cat = |c: &str| cats.contains(&c);
        // the first category is the main one
        d.kind = if cats.first() == Some(&"Blinder") || lname.contains("blinder") {
            FixtureKind::Blinder
        } else if has_pan || cat("Moving Head") || cat("Scanner") || cat("Barrel Scanner") {
            if lname.contains("beam") {
                FixtureKind::MovingBeam
            } else if lname.contains("wash") || (d.has_rgb() && !d.has(K::Gobo)) {
                FixtureKind::MovingWash
            } else {
                FixtureKind::MovingSpot
            }
        } else if cat("Pixel Bar") || lname.contains(" bar") {
            FixtureKind::LedBar
        } else if cats.first() == Some(&"Strobe") {
            FixtureKind::Strobe
        } else if cat("Color Changer") {
            FixtureKind::Par
        } else {
            FixtureKind::Other
        };
        if !d.channels.is_empty() {
            out.push(d);
        }
    }
    (out, unresolved)
}

fn load_parts(path: &Path) -> Result<(Vec<FixtureDef>, Vec<String>), String> {
    let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    let fx: Value = serde_json::from_str(&text).map_err(|e| format!("JSON non valido: {e}"))?;
    let fixture_key = path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    let man_dir = path.parent().ok_or("percorso non valido")?;
    let man_key = man_dir.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    let manufacturer = man_dir.parent().map(manufacturer_names).and_then(|m| m.get(&man_key).cloned()).unwrap_or_else(|| man_key.clone());
    // <library>/fixtures/<manufacturer>/<fixture>.json
    let version = man_dir.parent().and_then(|f| f.parent()).and_then(read_info).map(|i| i.sha).unwrap_or_default();
    let (defs, unresolved) = convert(&fx, &man_key, &manufacturer, &fixture_key, &version);
    if defs.is_empty() {
        return Err("Nessun modo DMX nel file".into());
    }
    Ok((defs, unresolved))
}

/// Parse one OFL fixture file: one FixtureDef per mode.
pub fn load(path: &Path) -> Result<Vec<FixtureDef>, String> {
    load_parts(path).map(|x| x.0)
}

/// For the `--test-ofl` check: the definitions and the channel names not found.
pub fn load_report(path: &Path) -> Result<(Vec<FixtureDef>, Vec<String>), String> {
    load_parts(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_moving_head() {
        let fx: Value = serde_json::from_str(
            r##"{
            "name": "Spot 1", "categories": ["Moving Head"],
            "physical": { "lens": { "degreesMinMax": [10, 20] } },
            "wheels": { "Color Wheel": { "slots": [ { "type": "Open" }, { "type": "Color", "name": "Red", "colors": ["#ff0000"] } ] } },
            "availableChannels": {
                "Pan": { "fineChannelAliases": ["Pan fine"], "capability": { "type": "Pan", "angleStart": "0deg", "angleEnd": "540deg" } },
                "Tilt": { "capability": { "type": "Tilt", "angleStart": "0deg", "angleEnd": "270deg" } },
                "Dimmer": { "capability": { "type": "Intensity" } },
                "Shutter": { "capabilities": [
                    { "dmxRange": [0, 9], "type": "ShutterStrobe", "shutterEffect": "Closed" },
                    { "dmxRange": [10, 19], "type": "ShutterStrobe", "shutterEffect": "Open" },
                    { "dmxRange": [20, 255], "type": "ShutterStrobe", "shutterEffect": "Strobe", "speedStart": "20Hz", "speedEnd": "1Hz" } ] },
                "Color Wheel": { "capabilities": [
                    { "dmxRange": [0, 9], "type": "WheelSlot", "slotNumber": 1 },
                    { "dmxRange": [10, 19], "type": "WheelSlot", "slotNumber": 2 },
                    { "dmxRange": [20, 255], "type": "WheelRotation", "speedStart": "slow CW", "speedEnd": "fast CW" } ] },
                "Speed": { "capability": { "type": "PanTiltSpeed", "speedStart": "fast", "speedEnd": "slow" } }
            },
            "modes": [ { "name": "8ch", "channels": ["Pan", "Pan fine", "Tilt", "Dimmer", "Shutter", "Color Wheel", "Speed", null] } ]
        }"##,
        )
        .unwrap();
        let (defs, missing) = convert(&fx, "acme", "Acme", "spot-1", "abc");
        assert!(missing.is_empty());
        let d = &defs[0];
        assert_eq!(d.id, "ofl:acme/spot-1|8ch");
        assert_eq!(d.kind, FixtureKind::MovingSpot);
        let kinds: Vec<K> = d.channels.iter().map(|c| c.kind).collect();
        assert_eq!(kinds, vec![K::Pan, K::PanFine, K::Tilt, K::Dimmer, K::Shutter, K::ColorWheel, K::PanTiltSpeed, K::Other]);
        assert_eq!(d.pan_range, 540.0);
        assert_eq!(d.beam_angle, 20.0);
        assert_eq!((d.shutter_open, d.shutter_closed), (14, 0));
        assert_eq!((d.strobe_min, d.strobe_max), (255, 20));
        assert_eq!(d.color_wheel.len(), 2);
        assert_eq!(d.color_wheel[1].color, [255, 0, 0]);
        assert_eq!(d.channels[6].default, 0);
        // capabilities, wheels and data sheet are kept and survive saving the project
        let sh = &d.channels[4].caps;
        assert_eq!(sh.len(), 3);
        assert_eq!((sh[2].start, sh[2].end, sh[2].what.as_str(), sh[2].values.as_str()), (20, 255, "Strobe", "20Hz → 1Hz"));
        let cw = &d.channels[5].caps;
        assert_eq!((cw[1].what.as_str(), cw[1].colors.clone()), ("Color Wheel: Red", vec![[255, 0, 0]]));
        assert_eq!(d.channels[1].caps[0].typ, "Fine");
        assert_eq!(d.wheels[0].slots.len(), 2);
        let info = d.info.as_ref().unwrap();
        assert_eq!((info.key.as_str(), info.mode.as_str(), info.version.as_str()), ("acme/spot-1", "8ch", "abc"));
        assert_eq!(info.lens_degrees, Some([10.0, 20.0]));
        let back: FixtureDef = serde_json::from_str(&serde_json::to_string(d).unwrap()).unwrap();
        assert_eq!(&back, d);
    }

    #[test]
    fn expands_matrix() {
        let fx: Value = serde_json::from_str(
            r#"{
            "name": "Bar", "categories": ["Pixel Bar"],
            "matrix": { "pixelCount": [3, 1, 1] },
            "availableChannels": {},
            "templateChannels": {
                "Red $pixelKey": { "capability": { "type": "ColorIntensity", "color": "Red" } },
                "Green $pixelKey": { "capability": { "type": "ColorIntensity", "color": "Green" } }
            },
            "modes": [
                { "name": "6ch", "channels": [ { "insert": "matrixChannels", "repeatFor": "eachPixelXYZ", "channelOrder": "perPixel", "templateChannels": ["Red $pixelKey", "Green $pixelKey"] } ] },
                { "name": "direct", "channels": ["Red 2", "Green 3"] }
            ]
        }"#,
        )
        .unwrap();
        let (defs, missing) = convert(&fx, "acme", "Acme", "bar", "");
        assert!(missing.is_empty(), "{missing:?}");
        assert_eq!(defs[0].channels.len(), 6);
        assert_eq!(defs[0].channels[2].kind, K::Red);
        assert_eq!(defs[0].kind, FixtureKind::LedBar);
        assert_eq!(defs[1].channels[1].kind, K::Green);
    }
}
