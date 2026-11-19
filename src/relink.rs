//! "Ricollega tracce": re-point the audio files of a project to a new folder
//! (typically after moving the project to another computer).

use crate::model::Song;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

const MAX_DEPTH: usize = 10;
const MAX_FILES: usize = 100_000;

#[derive(Default, Debug, PartialEq)]
pub struct RelinkReport {
    pub relinked: usize,
    pub already_ok: usize,
    pub missing: usize,
}

/// True when the track can be read: demo tracks are synthetic, others must exist on disk.
pub fn track_available(path: &str) -> bool {
    path.starts_with("demo:") || Path::new(path).is_file()
}

/// Does at least one track of the song exist on this computer?
pub fn song_has_audio(s: &Song) -> bool {
    s.tracks.iter().any(|t| track_available(&t.path))
}

/// Path components, accepting both `\` and `/` (the project may come from another OS).
fn comps(p: &str) -> Vec<String> {
    p.split(['/', '\\']).filter(|c| !c.is_empty()).map(|c| c.to_lowercase()).collect()
}

fn index_dir(root: &Path) -> HashMap<String, Vec<PathBuf>> {
    let mut idx: HashMap<String, Vec<PathBuf>> = HashMap::new();
    let mut stack = vec![(root.to_path_buf(), 0usize)];
    let mut n = 0usize;
    while let Some((dir, depth)) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                if depth < MAX_DEPTH {
                    stack.push((p, depth + 1));
                }
            } else if let Some(name) = p.file_name().map(|s| s.to_string_lossy().to_lowercase()) {
                n += 1;
                if n > MAX_FILES {
                    return idx;
                }
                idx.entry(name).or_default().push(p);
            }
        }
    }
    for v in idx.values_mut() {
        v.sort();
    }
    idx
}

/// Best candidate = the one sharing the longest tail of path components with the old path
/// (so `SongA/drums.mp3` and `SongB/drums.mp3` are told apart).
fn best_match<'a>(old: &[String], cands: &'a [PathBuf]) -> Option<&'a PathBuf> {
    let score = |c: &PathBuf| -> usize {
        let cc = comps(&c.to_string_lossy());
        old.iter().rev().zip(cc.iter().rev()).take_while(|(a, b)| a == b).count()
    };
    cands.iter().max_by_key(|c| (score(c), std::cmp::Reverse(c.to_string_lossy().len())))
}

/// Re-point every track of every song to the files found under `root`.
/// Tracks whose current file still exists are left alone.
pub fn relink(songs: &mut [Song], root: &Path) -> RelinkReport {
    let idx = index_dir(root);
    let mut rep = RelinkReport::default();
    for s in songs.iter_mut() {
        for t in s.tracks.iter_mut() {
            if t.path.starts_with("demo:") || Path::new(&t.path).is_file() {
                rep.already_ok += 1;
                continue;
            }
            let old = comps(&t.path);
            let Some(file) = old.last() else {
                rep.missing += 1;
                continue;
            };
            match idx.get(file).and_then(|c| best_match(&old, c)) {
                Some(p) => {
                    t.path = p.display().to_string();
                    rep.relinked += 1;
                }
                None => rep.missing += 1,
            }
        }
    }
    rep
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{TrackInfo, TrackRole};

    fn tr(path: &str) -> TrackInfo {
        TrackInfo { path: path.into(), name: "x".into(), role: TrackRole::guess("x"), env: vec![], hidden: false }
    }

    #[test]
    fn relinks_by_name_and_folder() {
        let root = std::env::temp_dir().join(format!("lightscio_relink_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for d in ["SongA", "SongB"] {
            std::fs::create_dir_all(root.join("sub").join(d)).unwrap();
            std::fs::write(root.join("sub").join(d).join("Drums.mp3"), b"x").unwrap();
        }
        let mut a = Song::new("A", 0);
        a.tracks = vec![tr(r"D:\Old\SongA\drums.mp3"), tr("/home/u/gone/SongB/Drums.mp3"), tr(r"D:\Old\Nope\bass.mp3")];
        let mut songs = vec![a];
        let rep = relink(&mut songs, &root);
        assert_eq!(rep, RelinkReport { relinked: 2, already_ok: 0, missing: 1 });
        assert!(songs[0].tracks[0].path.contains("SongA"));
        assert!(songs[0].tracks[1].path.contains("SongB"));
        assert_eq!(songs[0].tracks[2].path, r"D:\Old\Nope\bass.mp3");
        let _ = std::fs::remove_dir_all(&root);
    }
}
