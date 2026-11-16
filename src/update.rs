//! Update check: looks at the latest GitHub release of the project and, if it
//! is newer, downloads the installer for this platform and starts it.

use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

pub const DEFAULT_REPO: &str = "thomasborgogno/LightScio";

/// Version of this build, taken from the git tag (see build.rs).
pub fn current_version() -> &'static str {
    env!("APP_VERSION")
}

#[derive(Clone, Debug)]
pub struct ReleaseInfo {
    pub version: String,
    pub notes: String,
    pub page: String,
    pub asset_url: Option<String>,
    pub asset_name: String,
    pub asset_size: u64,
}

/// "https://github.com/owner/repo" or "owner/repo" -> "owner/repo".
pub fn normalize_repo(s: &str) -> String {
    let s = s.trim().trim_end_matches('/').trim_end_matches(".git");
    let s = s.split("github.com/").last().unwrap_or(s);
    s.split('/').take(2).collect::<Vec<_>>().join("/")
}

fn parse_version(s: &str) -> Vec<u64> {
    s.trim()
        .trim_start_matches(['v', 'V'])
        .split(['.', '-', '+'])
        .take(3)
        .map(|p| p.chars().take_while(|c| c.is_ascii_digit()).collect::<String>().parse().unwrap_or(0))
        .collect()
}

pub fn is_newer(remote: &str, local: &str) -> bool {
    let (mut r, mut l) = (parse_version(remote), parse_version(local));
    r.resize(3, 0);
    l.resize(3, 0);
    r > l
}

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new().timeout_connect(std::time::Duration::from_secs(8)).timeout_read(std::time::Duration::from_secs(30)).build()
}

/// The installer that fits this computer, among the release files.
fn pick_asset(names: &[String]) -> Option<usize> {
    let lower: Vec<String> = names.iter().map(|n| n.to_lowercase()).collect();
    if cfg!(target_os = "windows") {
        lower.iter().position(|n| n.ends_with(".exe") && n.contains("setup")).or_else(|| lower.iter().position(|n| n.ends_with(".exe") || n.ends_with(".msi")))
    } else if cfg!(target_os = "macos") {
        lower.iter().position(|n| n.ends_with(".dmg"))
    } else {
        None
    }
}

/// Ask GitHub for the latest release. Ok(None) = already up to date.
pub fn check(repo: &str) -> Result<Option<ReleaseInfo>, String> {
    let repo = normalize_repo(repo);
    if !repo.contains('/') {
        return Err("indirizzo del progetto non valido (serve utente/repository)".into());
    }
    let url = format!("https://api.github.com/repos/{repo}/releases/latest");
    let direct = agent()
        .get(&url)
        .set("User-Agent", "LightScio-update-check")
        .set("Accept", "application/vnd.github+json")
        .call();
    let v: serde_json::Value = match direct {
        Ok(resp) => resp.into_json().map_err(|e| format!("risposta non valida: {e}"))?,
        Err(ureq::Error::Status(404, _)) => return Err("nessuna versione pubblicata (ancora)".into()),
        Err(ureq::Error::Status(c, _)) => return Err(format!("il server ha risposto {c}")),
        Err(ureq::Error::Transport(t)) => {
            // fall back to the system curl (uses the system network setup)
            let text = curl_text(&url).map_err(|e| format!("connessione non riuscita ({t}; {e})"))?;
            let v: serde_json::Value = serde_json::from_str(&text).map_err(|e| format!("risposta non valida: {e}"))?;
            if v.get("message").and_then(|m| m.as_str()) == Some("Not Found") {
                return Err("nessuna versione pubblicata (ancora)".into());
            }
            v
        }
    };
    let tag = v.get("tag_name").and_then(|x| x.as_str()).unwrap_or("").to_string();
    if tag.is_empty() || !is_newer(&tag, current_version()) {
        return Ok(None);
    }
    let assets = v.get("assets").and_then(|a| a.as_array()).cloned().unwrap_or_default();
    let names: Vec<String> = assets.iter().map(|a| a.get("name").and_then(|x| x.as_str()).unwrap_or("").to_string()).collect();
    let pick = pick_asset(&names);
    Ok(Some(ReleaseInfo {
        version: tag.trim_start_matches(['v', 'V']).to_string(),
        notes: v.get("body").and_then(|x| x.as_str()).unwrap_or("").to_string(),
        page: v.get("html_url").and_then(|x| x.as_str()).unwrap_or("").to_string(),
        asset_url: pick.and_then(|i| assets[i].get("browser_download_url").and_then(|x| x.as_str()).map(|s| s.to_string())),
        asset_name: pick.map(|i| names[i].clone()).unwrap_or_default(),
        asset_size: pick.and_then(|i| assets[i].get("size").and_then(|x| x.as_u64())).unwrap_or(0),
    }))
}

fn curl_cmd() -> std::process::Command {
    let mut c = std::process::Command::new(if cfg!(windows) { "curl.exe" } else { "curl" });
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x0800_0000);
    }
    c
}

fn curl_text(url: &str) -> Result<String, String> {
    let out = curl_cmd()
        .args(["-sSL", "--max-time", "30", "-H", "User-Agent: LightScio-update-check", "-H", "Accept: application/vnd.github+json", url])
        .output()
        .map_err(|e| format!("curl non disponibile: {e}"))?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    Ok(String::from_utf8_lossy(&out.stdout).to_string())
}

/// Download with the system curl, following the file size for the progress bar.
fn curl_download(url: &str, path: &std::path::Path, total: u64, progress: &Arc<Mutex<f32>>) -> Result<(), String> {
    let mut child = curl_cmd()
        .args(["-sSL", "--fail", "-H", "User-Agent: LightScio-update-check", "-o"])
        .arg(path)
        .arg(url)
        .spawn()
        .map_err(|e| format!("curl non disponibile: {e}"))?;
    loop {
        if let Some(st) = child.try_wait().map_err(|e| e.to_string())? {
            if !st.success() {
                return Err("download non riuscito".into());
            }
            return Ok(());
        }
        let size = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
        if let Ok(mut p) = progress.lock() {
            *p = (size as f32 / total.max(1) as f32).min(0.99);
        }
        std::thread::sleep(std::time::Duration::from_millis(200));
    }
}

/// Download the installer into the temp folder, reporting progress 0..1.
pub fn download(info: &ReleaseInfo, progress: Arc<Mutex<f32>>) -> Result<PathBuf, String> {
    let url = info.asset_url.clone().ok_or("nessun installer per questo sistema in questa versione")?;
    let dir = std::env::temp_dir().join("LightScio-update");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let name: String = info.asset_name.chars().filter(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_')).collect();
    let path = dir.join(if name.is_empty() { "LightScio-update".to_string() } else { name });
    let resp = match agent().get(&url).set("User-Agent", "LightScio-update-check").call() {
        Ok(r) => r,
        Err(ureq::Error::Transport(_)) => {
            curl_download(&url, &path, info.asset_size, &progress)?;
            if let Ok(mut p) = progress.lock() {
                *p = 1.0;
            }
            return Ok(path);
        }
        Err(e) => return Err(format!("download non riuscito: {e}")),
    };
    let total = resp.header("Content-Length").and_then(|s| s.parse::<u64>().ok()).unwrap_or(info.asset_size).max(1);
    let mut file = std::fs::File::create(&path).map_err(|e| e.to_string())?;
    let mut reader = resp.into_reader();
    let mut buf = vec![0u8; 64 * 1024];
    let mut done = 0u64;
    loop {
        let n = reader.read(&mut buf).map_err(|e| format!("download interrotto: {e}"))?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n]).map_err(|e| e.to_string())?;
        done += n as u64;
        if let Ok(mut p) = progress.lock() {
            *p = (done as f32 / total as f32).min(1.0);
        }
    }
    if done < 1024 {
        return Err("file scaricato non valido".into());
    }
    Ok(path)
}

/// Start the downloaded installer. On Windows the app should close right after.
pub fn launch_installer(path: &std::path::Path) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        // Inno Setup: show progress only, close the running app; the installer starts it again at the end
        std::process::Command::new(path)
            .args(["/SILENT", "/CLOSEAPPLICATIONS", "/NORESTART"])
            .spawn()
            .map_err(|e| format!("impossibile avviare l'installer: {e}"))?;
        Ok(())
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open").arg(path).spawn().map_err(|e| format!("impossibile aprire {}: {e}", path.display()))?;
        Ok(())
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        Err(format!("installa a mano il file scaricato: {}", path.display()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn versions() {
        assert!(is_newer("v0.2.0", "0.1.9"));
        assert!(is_newer("1.0.0", "0.9.9"));
        assert!(!is_newer("v0.1.0", "0.1.0"));
        assert!(!is_newer("0.1.0-beta", "0.1.0"));
        assert_eq!(normalize_repo("https://github.com/thomasborgogno/LightScio"), "thomasborgogno/LightScio");
        assert_eq!(normalize_repo("thomasborgogno/LightScio/"), "thomasborgogno/LightScio");
    }
}
