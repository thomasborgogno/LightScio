//! Start LightSciò with a given show when the user logs in (rack computers).
//! Windows: HKCU\Software\Microsoft\Windows\CurrentVersion\Run
//! macOS:   ~/Library/LaunchAgents/io.github.thomasborgogno.lightscio.plist
//! Linux:   ~/.config/autostart/lightscio.desktop

use std::path::{Path, PathBuf};

const NAME: &str = "LightScio";

fn exe() -> Result<PathBuf, String> {
    std::env::current_exe().map_err(|e| e.to_string())
}

#[cfg(windows)]
fn reg(args: &[&str]) -> std::io::Result<std::process::Output> {
    use std::os::windows::process::CommandExt;
    std::process::Command::new("reg").args(args).creation_flags(0x0800_0000).output()
}

#[cfg(windows)]
const RUN_KEY: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";

/// The show that is started at login, if any.
pub fn current() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        let out = reg(&["query", RUN_KEY, "/v", NAME]).ok()?;
        if !out.status.success() {
            return None;
        }
        let text = String::from_utf8_lossy(&out.stdout).to_string();
        let line = text.lines().find(|l| l.contains("REG_SZ"))?;
        let value = line.split("REG_SZ").nth(1)?.trim().to_string();
        // value: "C:\...\lightscio.exe" "C:\...\show.luci"
        let parts: Vec<&str> = value.split('"').filter(|s| !s.trim().is_empty()).collect();
        return parts.get(1).map(PathBuf::from);
    }
    #[cfg(target_os = "macos")]
    {
        let text = std::fs::read_to_string(plist_path()?).ok()?;
        let strings: Vec<&str> = text.split("<string>").skip(1).filter_map(|s| s.split("</string>").next()).collect();
        return strings.iter().find(|s| s.ends_with(".luci")).map(|s| PathBuf::from(unescape_xml(s)));
    }
    #[cfg(all(not(windows), not(target_os = "macos")))]
    {
        let text = std::fs::read_to_string(desktop_path()?).ok()?;
        let line = text.lines().find(|l| l.starts_with("Exec="))?;
        return line.split('"').filter(|s| s.ends_with(".luci")).map(PathBuf::from).next();
    }
}

pub fn enable(show: &Path) -> Result<(), String> {
    let exe = exe()?;
    #[cfg(windows)]
    {
        let value = format!("\"{}\" \"{}\"", exe.display(), show.display());
        let out = reg(&["add", RUN_KEY, "/v", NAME, "/t", "REG_SZ", "/d", &value, "/f"]).map_err(|e| e.to_string())?;
        if !out.status.success() {
            return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
        }
        return Ok(());
    }
    #[cfg(target_os = "macos")]
    {
        let p = plist_path().ok_or("cartella utente non trovata")?;
        if let Some(d) = p.parent() {
            std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
        }
        let plist = format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key><string>io.github.thomasborgogno.lightscio</string>
  <key>ProgramArguments</key>
  <array>
    <string>{}</string>
    <string>{}</string>
  </array>
  <key>RunAtLoad</key><true/>
  <key>ProcessType</key><string>Interactive</string>
</dict>
</plist>
"#,
            escape_xml(&exe.display().to_string()),
            escape_xml(&show.display().to_string())
        );
        return std::fs::write(&p, plist).map_err(|e| e.to_string());
    }
    #[cfg(all(not(windows), not(target_os = "macos")))]
    {
        let p = desktop_path().ok_or("cartella utente non trovata")?;
        if let Some(d) = p.parent() {
            std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
        }
        let text = format!("[Desktop Entry]\nType=Application\nName=LightScio\nExec=\"{}\" \"{}\"\nX-GNOME-Autostart-enabled=true\n", exe.display(), show.display());
        return std::fs::write(&p, text).map_err(|e| e.to_string());
    }
}

pub fn disable() -> Result<(), String> {
    #[cfg(windows)]
    {
        if current().is_some() {
            let out = reg(&["delete", RUN_KEY, "/v", NAME, "/f"]).map_err(|e| e.to_string())?;
            if !out.status.success() {
                return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
            }
        }
        return Ok(());
    }
    #[cfg(target_os = "macos")]
    {
        if let Some(p) = plist_path() {
            if p.exists() {
                std::fs::remove_file(p).map_err(|e| e.to_string())?;
            }
        }
        return Ok(());
    }
    #[cfg(all(not(windows), not(target_os = "macos")))]
    {
        if let Some(p) = desktop_path() {
            if p.exists() {
                std::fs::remove_file(p).map_err(|e| e.to_string())?;
            }
        }
        return Ok(());
    }
}

#[cfg(target_os = "macos")]
fn plist_path() -> Option<PathBuf> {
    std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Library/LaunchAgents/io.github.thomasborgogno.lightscio.plist"))
}

#[cfg(target_os = "macos")]
fn escape_xml(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

#[cfg(target_os = "macos")]
fn unescape_xml(s: &str) -> String {
    s.replace("&lt;", "<").replace("&gt;", ">").replace("&amp;", "&")
}

#[cfg(all(not(windows), not(target_os = "macos")))]
fn desktop_path() -> Option<PathBuf> {
    std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config/autostart/lightscio.desktop"))
}
