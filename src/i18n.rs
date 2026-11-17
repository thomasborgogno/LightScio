//! Translations. The interface is written in Italian; when English is chosen
//! every text goes through a dictionary (src/i18n_en.txt, lines italiano ||| English):
//! - `ts("literal")` for fixed texts (exact match, returns &'static str);
//! - `t(&format!(..))` for texts with values inside: the dictionary holds
//!   templates like `Aggiunti {} fari` -> `Added {0} fixtures`.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};

static ENGLISH: AtomicBool = AtomicBool::new(false);

pub fn set_english(en: bool) {
    ENGLISH.store(en, Ordering::Relaxed);
    if let Ok(mut m) = memo().lock() {
        m.clear();
    }
}

pub fn is_english() -> bool {
    ENGLISH.load(Ordering::Relaxed)
}

/// "it" / "en" / "auto" -> English?
pub fn resolve(pref: &str) -> bool {
    match pref {
        "it" => false,
        "en" => true,
        _ => !sys_locale::get_locale().map(|l| l.to_lowercase().starts_with("it")).unwrap_or(true),
    }
}

struct Template {
    parts: Vec<&'static str>,
    en: &'static str,
}

struct Table {
    exact: HashMap<&'static str, &'static str>,
    templates: Vec<Template>,
}

fn unescape(s: &str) -> String {
    s.replace("\\n", "\n").replace("\\t", "\t").replace("\\\"", "\"").replace("\\\\", "\\")
}

fn table() -> &'static Table {
    static T: OnceLock<Table> = OnceLock::new();
    T.get_or_init(|| {
        let mut exact = HashMap::new();
        let mut templates = vec![];
        for line in include_str!("i18n_en.txt").lines() {
            if line.trim().is_empty() || line.starts_with('#') {
                continue;
            }
            let Some((it, en)) = line.split_once(" ||| ") else { continue };
            let it: &'static str = Box::leak(unescape(it).into_boxed_str());
            let en: &'static str = Box::leak(unescape(en.trim_end_matches('\r')).into_boxed_str());
            if it.contains("{}") {
                let parts: Vec<&'static str> = it.split("{}").collect();
                // two placeholders in a row cannot be told apart
                if parts.iter().skip(1).take(parts.len().saturating_sub(2)).any(|p| p.is_empty()) {
                    continue;
                }
                templates.push(Template { parts, en });
            } else {
                exact.insert(it, en);
            }
        }
        // longer templates first (more specific)
        templates.sort_by_key(|t| std::cmp::Reverse(t.parts.iter().map(|p| p.len()).sum::<usize>()));
        Table { exact, templates }
    })
}

fn memo() -> &'static Mutex<HashMap<String, String>> {
    static M: OnceLock<Mutex<HashMap<String, String>>> = OnceLock::new();
    M.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Fixed text.
pub fn ts(s: &'static str) -> &'static str {
    if !is_english() {
        return s;
    }
    table().exact.get(s).copied().unwrap_or(s)
}

fn match_template(parts: &[&str], s: &str) -> Option<Vec<String>> {
    let first = parts[0];
    let mut rest = s.strip_prefix(first)?;
    let mut caps = vec![];
    for (i, p) in parts.iter().enumerate().skip(1) {
        if i == parts.len() - 1 {
            let cap = rest.strip_suffix(p)?;
            caps.push(cap.to_string());
            rest = "";
        } else {
            let k = rest.find(p)?;
            caps.push(rest[..k].to_string());
            rest = &rest[k + p.len()..];
        }
    }
    let _ = rest;
    Some(caps)
}

fn fill(en: &str, caps: &[String]) -> String {
    let mut out = String::with_capacity(en.len() + 16);
    let mut seq = 0usize;
    let mut chars = en.char_indices().peekable();
    let b = en.as_bytes();
    while let Some((i, c)) = chars.next() {
        if c == '{' {
            if let Some(j) = en[i + 1..].find('}') {
                let inner = &en[i + 1..i + 1 + j];
                let idx = if inner.is_empty() {
                    seq += 1;
                    Some(seq - 1)
                } else {
                    inner.parse::<usize>().ok()
                };
                if let Some(k) = idx {
                    out.push_str(caps.get(k).map(|s| s.as_str()).unwrap_or(""));
                    for _ in 0..=j {
                        chars.next();
                    }
                    continue;
                }
            }
        }
        let _ = b;
        out.push(c);
    }
    out
}

fn translate(s: &str, depth: u8) -> String {
    let tab = table();
    if let Some(e) = tab.exact.get(s) {
        return e.to_string();
    }
    if depth == 0 {
        return s.to_string();
    }
    for tp in &tab.templates {
        if let Some(caps) = match_template(&tp.parts, s) {
            let caps: Vec<String> = caps.iter().map(|c| translate(c, depth - 1)).collect();
            return fill(tp.en, &caps);
        }
    }
    s.to_string()
}

/// Text with values inside (or coming from elsewhere: status, logs, errors).
pub fn t(s: &str) -> String {
    if !is_english() {
        return s.to_string();
    }
    if let Some(e) = table().exact.get(s) {
        return e.to_string();
    }
    if let Ok(m) = memo().lock() {
        if let Some(v) = m.get(s) {
            return v.clone();
        }
    }
    let out = translate(s, 2);
    if let Ok(mut m) = memo().lock() {
        if m.len() > 4000 {
            m.clear();
        }
        m.insert(s.to_string(), out.clone());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn templates() {
        let parts = vec!["Aggiunti ", " × ", ""];
        assert_eq!(match_template(&parts, "Aggiunti 3 × PAR").unwrap(), vec!["3".to_string(), "PAR".to_string()]);
        assert_eq!(fill("Added {0} × {1}", &["3".into(), "PAR".into()]), "Added 3 × PAR");
        assert_eq!(fill("{1} then {0}", &["a".into(), "b".into()]), "b then a");
        assert_eq!(fill("x {} y {}", &["a".into(), "b".into()]), "x a y b");
    }
}
