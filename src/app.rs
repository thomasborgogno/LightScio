//! Main application state, background jobs, project files and top bar.

use crate::analysis::{self, AnalysisOut, LoadOutput, Progress, ProgressHandle};
#[allow(unused_imports)]
use crate::i18n::{t, ts};
use crate::audio::Player;
use crate::autoshow;
use crate::dmx::{self, PortInfo};
use crate::engine::{self, FxState};
use crate::live::{Shared, SharedHandle};
use crate::midi::MidiIn;
use crate::model::*;
use egui::{Color32, RichText};
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Rig,
    Songs,
    Show,
    Live,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Sel {
    None,
    Section(usize),
    Cue(usize),
    Hit(usize),
}

#[derive(Clone, Copy, PartialEq)]
pub enum TlDrag {
    Cue(usize),
    Hit(usize),
    Boundary(usize),
    Seek,
    /// Lane being resized: lane, pointer y at start, height at start, lanes of that kind above.
    Resize(crate::lanes::Lane, f32, f32, usize),
}

pub struct Timeline {
    pub pps: f32,
    pub start: f64,
    pub follow: bool,
    pub snap: bool,
    pub add_hit: HitKind,
    pub drag: Option<TlDrag>,
    /// Hits can be added / moved / deleted only in this mode.
    pub hit_mode: bool,
    /// Offset between the pointer and the start of the dragged item.
    pub grab: f64,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum StageMode {
    Perspective,
    Plan,
}

pub enum JobResult {
    Loaded { song: usize, analyze: bool, out: Result<LoadOutput, String> },
    Claude { song: usize, out: Result<(Vec<Cue>, Vec<Hit>), String> },
}

pub struct Job {
    pub label: String,
    pub progress: ProgressHandle,
    pub rx: Receiver<JobResult>,
    pub started: Instant,
}

pub struct Shot {
    pub path: PathBuf,
    pub requested: bool,
}

pub struct App {
    pub project: Project,
    pub path: Option<PathBuf>,
    pub catalog: Vec<FixtureDef>,
    pub tab: Tab,
    pub song: usize,
    pub sel: Sel,
    pub sel_fixture: Option<u32>,
    pub dragging: Option<u32>,
    pub shared: SharedHandle,
    pub player: Player,
    pub audio_for: Option<usize>,
    pub midi: MidiIn,
    pub midi_ports: Vec<String>,
    pub midi_choice: String,
    pub dmx_ports: Vec<PortInfo>,
    pub dmx_choice: String,
    pub jobs: Vec<Job>,
    pub status: String,
    pub tl: Timeline,
    pub edit_def: Option<FixtureDef>,
    pub catalog_pick: String,
    pub add_count: u32,
    pub last_push: Instant,
    pub last_gen: u64,
    pub last_realign: Instant,
    pub screenshot: Option<Shot>,
    pub frame: u64,
    pub stage_mode: StageMode,
    pub undo: Vec<(usize, Song)>,
    pub show_all_groups: bool,
    pub test_all: bool,
    pub states: Vec<FxState>,
    pub bpb_for_analysis: u32,
    /// Multi-selection of fixtures (always contains `sel_fixture` when set).
    pub sel_set: std::collections::BTreeSet<u32>,
    pub plan_drag: Option<crate::stage::PlanDrag>,
    /// Favourite fixture models (kept across projects).
    pub favorites: Vec<FixtureDef>,
    pub qlc: Vec<crate::qlc::QlcEntry>,
    pub qlc_dirs: Vec<PathBuf>,
    pub qlc_rx: Option<Receiver<Vec<crate::qlc::QlcEntry>>>,
    pub qlc_cache: std::collections::HashMap<PathBuf, Result<Vec<FixtureDef>, String>>,
    /// Picked QLC+ fixture and mode index.
    pub qlc_pick: Option<(PathBuf, usize)>,
    pub search: String,
    /// Stop the preview at this time (listening to one part).
    pub play_until: Option<f64>,
    /// Play request waiting for the audio to be loaded: (from, until).
    pub pending_play: Option<(f64, Option<f64>)>,
    /// Song whose audio was already loaded automatically once (avoid retry loops).
    pub auto_audio_tried: Option<usize>,
    /// Waveform peaks per song (not saved; rebuilt when the audio is loaded).
    pub waves: std::collections::HashMap<usize, Vec<Vec<f32>>>,
    /// Analysis editor (Brani e analisi tab).
    pub ed: EditorState,
    /// Last grid sent to the metronome.
    pub metro_key: String,
    /// "Convert show for other lights" dialog.
    pub adapt: Option<crate::ui_convert::AdaptState>,
    /// Lighting setup waiting for confirmation (replace or add): setup, file name.
    pub rig_import: Option<(Rig, String)>,
    pub prefs: Prefs,
    pub show_prefs: bool,
    pub update: crate::ui_update::UpdateState,
    pub open_url: Option<String>,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum EdDrag {
    Seek,
    /// None = first downbeat, Some(i) = extra downbeat i.
    Anchor(Option<usize>),
    Boundary(usize),
    Resize(crate::lanes::Lane, f32, f32, usize),
}

pub struct EditorState {
    pub start: f64,
    pub pps: f32,
    pub drag: Option<EdDrag>,
    pub sel: Option<usize>,
    pub follow: bool,
}

pub fn fmt_time(t: f64) -> String {
    let t = t.max(0.0);
    let m = (t / 60.0).floor();
    let s = t - m * 60.0;
    format!("{:02}:{:05.2}", m as i64, s)
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>, screenshot: Option<PathBuf>, open_file: Option<PathBuf>) -> Self {
        // language first: everything created below uses it
        let stored_prefs: Option<Prefs> = cc.storage.and_then(|s| s.get_string("prefs")).and_then(|s| serde_json::from_str(&s).ok());
        let lang = std::env::var("LUCI_LANG").ok().or_else(|| stored_prefs.as_ref().map(|p| p.language.clone())).unwrap_or_else(|| "auto".into());
        crate::i18n::set_english(crate::i18n::resolve(&lang));
        setup_style(&cc.egui_ctx);
        let catalog = crate::catalog::builtin();
        let mut path = None;
        let mut project = None;
        // a show file opened by double click (file association set by the installer)
        if let Some(f) = open_file.filter(|_| screenshot.is_none()) {
            if let Ok(pr) = load_project(&f) {
                project = Some(pr);
                path = Some(f);
            }
        }
        if screenshot.is_none() && project.is_none() {
            if let Some(storage) = cc.storage {
                if let Some(p) = storage.get_string("last_project") {
                    let pb = PathBuf::from(p);
                    if let Ok(pr) = load_project(&pb) {
                        project = Some(pr);
                        path = Some(pb);
                    }
                }
            }
        }
        let mut audio = None;
        let mut demo_peaks: Option<Vec<Vec<f32>>> = None;
        let project = match project {
            Some(p) => p,
            None => {
                let (p, a, pk) = demo_project();
                audio = a;
                demo_peaks = Some(pk);
                p
            }
        };
        let shared = Shared::new(project.clone());
        dmx::spawn(shared.clone());
        let mut app = App {
            project,
            path,
            catalog,
            tab: Tab::Show,
            song: 0,
            sel: Sel::None,
            sel_fixture: None,
            dragging: None,
            shared,
            player: Player::new(),
            audio_for: None,
            midi: MidiIn::new(),
            midi_ports: MidiIn::ports(),
            midi_choice: String::new(),
            dmx_ports: dmx::ports(),
            dmx_choice: String::new(),
            jobs: vec![],
            status: ts("Pronto").into(),
            tl: Timeline {
                pps: 18.0,
                start: 0.0,
                follow: true,
                snap: true,
                add_hit: HitKind::Blinder,
                drag: None,
                hit_mode: false,
                grab: 0.0,
            },
            sel_set: Default::default(),
            plan_drag: None,
            favorites: vec![],
            qlc: vec![],
            qlc_dirs: vec![],
            qlc_rx: None,
            qlc_cache: Default::default(),
            qlc_pick: None,
            search: String::new(),
            play_until: None,
            pending_play: None,
            auto_audio_tried: None,
            waves: Default::default(),
            ed: EditorState { start: 0.0, pps: 22.0, drag: None, sel: None, follow: true },
            metro_key: String::new(),
            adapt: None,
            rig_import: None,
            prefs: Prefs::default(),
            show_prefs: false,
            update: Default::default(),
            open_url: None,
            edit_def: None,
            catalog_pick: "head-spot-11".into(),
            add_count: 1,
            last_push: Instant::now(),
            last_gen: 0,
            last_realign: Instant::now(),
            screenshot: screenshot.map(|path| Shot { path, requested: false }),
            frame: 0,
            stage_mode: StageMode::Perspective,
            undo: vec![],
            show_all_groups: false,
            test_all: false,
            states: vec![],
            bpb_for_analysis: 4,
        };
        if let Some(a) = audio {
            app.player.set_buffer(Some(a));
            app.audio_for = Some(0);
        }
        if let Some(pk) = demo_peaks {
            app.waves.insert(0, pk);
        }
        if let Some(storage) = cc.storage {
            if let Some(f) = storage.get_string("favorites").and_then(|s| serde_json::from_str::<Vec<FixtureDef>>(&s).ok()) {
                app.favorites = f;
            }
            if let Some(p) = storage.get_string("prefs").and_then(|s| serde_json::from_str::<Prefs>(&s).ok()) {
                apply_prefs(&cc.egui_ctx, &p);
                app.prefs = p;
            }
            if let Some(d) = storage.get_string("qlc_dirs").and_then(|s| serde_json::from_str::<Vec<PathBuf>>(&s).ok()) {
                app.qlc_dirs = d.into_iter().filter(|p| p.is_dir()).collect();
            }
        }
        if app.qlc_dirs.is_empty() {
            app.qlc_dirs = crate::qlc::default_dirs();
        }
        if app.screenshot.is_none() {
            app.start_qlc_scan();
            app.auto_update_check();
        }
        if app.midi_choice.is_empty() {
            app.midi_choice = app.midi_ports.first().cloned().unwrap_or_default();
        }
        if let Some(p) = app.dmx_ports.first() {
            app.dmx_choice = p.name.clone();
        }
        if app.screenshot.is_some() {
            // optional: LUCI_TAB=rig|songs|live to check other tabs
            match std::env::var("LUCI_TAB").unwrap_or_default().as_str() {
                "rig" => {
                    app.tab = Tab::Rig;
                    app.stage_mode = StageMode::Plan;
                    app.sel_set = app.project.fixtures.iter().filter(|f| f.group == Group::BackWash).map(|f| f.id).collect();
                    app.sel_fixture = app.sel_set.iter().next().copied();
                    app.qlc_dirs = crate::qlc::default_dirs();
                    app.qlc = crate::qlc::scan(&app.qlc_dirs);
                    app.search = "par".into();
                }
                "songs" => app.tab = Tab::Songs,
                "live" => app.tab = Tab::Live,
                _ => {}
            }
            // show a lively moment of the demo: the first chorus
            let mut t = app
                .project
                .songs
                .first()
                .and_then(|s| s.sections.iter().find(|x| x.kind == SectionKind::Chorus).map(|x| x.start + 4.0))
                .unwrap_or(60.0);
            if let Some(s) = app.project.songs.first() {
                // avoid a moment covered by a white flash / blinder
                while s.hits.iter().any(|h| h.time <= t && t < h.time + h.duration + 1.2) && t < s.duration() {
                    t += 0.25;
                }
            }
            app.tl.start = (t - 40.0).max(0.0);
            if let Ok(mut s) = app.shared.lock() {
                s.transport.play_from(t, Instant::now(), false);
            }
            if let Some(i) = app.project.songs.first().and_then(|s| s.cues.iter().rposition(|c| c.time <= t)) {
                app.sel = Sel::Cue(i);
            }
        }
        app.push_project();
        app
    }

    // ------------------------------------------------------------ transport

    pub fn now(&self) -> f64 {
        self.shared.lock().map(|s| s.transport.pos()).unwrap_or(0.0)
    }
    pub fn is_playing(&self) -> bool {
        self.shared.lock().map(|s| s.transport.playing).unwrap_or(false)
    }
    pub fn play(&mut self) {
        self.play_until = None;
        let pos = self.now();
        let dur = self.cur_song().map(|s| s.duration()).unwrap_or(0.0);
        let pos = if pos >= dur - 0.05 { 0.0 } else { pos };
        let has_tracks = self.cur_song().map(|s| !s.tracks.is_empty()).unwrap_or(false);
        if self.audio_for != Some(self.song) && has_tracks {
            // audio not in memory yet (e.g. project just reopened): load it, then start
            if !self.job_running("Carico") && !self.job_running("Analisi") {
                self.start_load(self.song, false);
            }
            self.pending_play = Some((pos, None));
            self.status = ts("Carico l'audio delle tracce, la riproduzione parte appena è pronto…").into();
            return;
        }
        if self.audio_for == Some(self.song) {
            self.player.play(pos);
            if !self.player.error.is_empty() {
                self.status = crate::i18n::t(&format!("Audio: {}", std::mem::take(&mut self.player.error)));
            }
        }
        if let Ok(mut s) = self.shared.lock() {
            s.transport.song = self.song;
            s.transport.play_from(pos, Instant::now(), false);
            self.last_gen = s.transport.gen;
        }
    }
    pub fn pause(&mut self) {
        self.player.stop();
        if let Ok(mut s) = self.shared.lock() {
            s.transport.pause();
            self.last_gen = s.transport.gen;
        }
    }
    pub fn stop(&mut self) {
        self.player.stop();
        if let Ok(mut s) = self.shared.lock() {
            s.transport.stop(false);
            self.last_gen = s.transport.gen;
        }
        self.tl.start = 0.0;
    }
    pub fn seek(&mut self, t: f64) {
        let t = t.max(0.0);
        if self.player.is_playing() {
            self.player.seek(t);
        }
        if let Ok(mut s) = self.shared.lock() {
            s.transport.seek(t);
            self.last_gen = s.transport.gen;
        }
    }
    pub fn select_song(&mut self, i: usize) {
        if i == self.song && self.shared.lock().map(|s| s.transport.song == i).unwrap_or(true) {
            return;
        }
        self.player.stop();
        self.song = i.min(self.project.songs.len().saturating_sub(1));
        self.sel = Sel::None;
        self.tl.start = 0.0;
        if let Ok(mut s) = self.shared.lock() {
            s.transport.song = self.song;
            s.transport.stop(false);
            self.last_gen = s.transport.gen;
        }
        if self.audio_for != Some(self.song) {
            self.player.set_buffer(None);
            self.audio_for = None; self.auto_audio_tried = None; self.waves.clear();
        }
    }

    pub fn cur_song(&self) -> Option<&Song> {
        self.project.songs.get(self.song)
    }
    pub fn cur_song_mut(&mut self) -> Option<&mut Song> {
        self.project.songs.get_mut(self.song)
    }

    pub fn push_undo(&mut self) {
        if let Some(s) = self.cur_song() {
            let snap = s.clone();
            self.undo.push((self.song, snap));
            if self.undo.len() > 40 {
                self.undo.remove(0);
            }
        }
    }
    pub fn do_undo(&mut self) {
        if let Some((i, s)) = self.undo.pop() {
            if let Some(dst) = self.project.songs.get_mut(i) {
                *dst = s;
                self.sel = Sel::None;
                self.status = ts("Modifica annullata").into();
            }
        }
    }

    pub fn push_project(&mut self) {
        if let Ok(mut s) = self.shared.lock() {
            if let Some(m) = s.pending_master.take() {
                self.project.settings.master = m;
            }
            s.project = Arc::new(self.project.clone());
            s.dmx.protocol = self.project.settings.dmx_protocol;
            s.dmx.fps = self.project.settings.dmx_fps;
        }
        self.last_push = Instant::now();
    }

    /// Play the preview between two times (listening to one part of the song).
    pub fn play_range(&mut self, from: f64, until: Option<f64>) {
        let has_tracks = self.cur_song().map(|s| !s.tracks.is_empty()).unwrap_or(false);
        if self.audio_for != Some(self.song) && has_tracks {
            if !self.job_running("Carico") && !self.job_running("Analisi") {
                self.start_load(self.song, false);
            }
            self.pending_play = Some((from, until));
            self.status = ts("Carico l'audio del brano, parte tra un attimo…").into();
            return;
        }
        self.player.stop();
        self.seek(from.max(0.0));
        self.play();
        self.play_until = until;
    }

    pub fn start_qlc_scan(&mut self) {
        let dirs = self.qlc_dirs.clone();
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(crate::qlc::scan(&dirs));
        });
        self.qlc_rx = Some(rx);
    }

    // ------------------------------------------------------------ jobs

    pub fn job_running(&self, label_prefix: &str) -> bool {
        self.jobs.iter().any(|j| j.label.starts_with(label_prefix))
    }

    pub fn start_load(&mut self, song: usize, analyze: bool) {
        let Some(s) = self.project.songs.get(song) else { return };
        if s.tracks.is_empty() {
            self.status = ts("Aggiungi prima le tracce audio del brano").into();
            return;
        }
        let tracks = s.tracks.clone();
        let bpb = self.bpb_for_analysis;
        let progress: ProgressHandle = Arc::new(Mutex::new(Progress::default()));
        let p2 = progress.clone();
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let out = analysis::load_song(&tracks, analyze, bpb, &p2);
            let _ = tx.send(JobResult::Loaded { song, analyze, out });
        });
        let label = if analyze { format!("Analisi di «{}»", s.name) } else { format!("Carico l'audio di «{}»", s.name) };
        self.jobs.push(Job { label, progress, rx, started: Instant::now() });
    }

    pub fn start_claude(&mut self, song: usize) {
        let Some(s) = self.project.songs.get(song) else { return };
        if s.analysis.is_none() {
            self.status = ts("Analizza prima il brano").into();
            return;
        }
        let prompt = autoshow::claude_prompt(s, &self.project, &self.catalog);
        let cmd = self.project.settings.claude_command.clone();
        let song_copy = s.clone();
        let progress: ProgressHandle = Arc::new(Mutex::new(Progress { frac: 0.0, msg: ts("Claude Code sta progettando lo show…").into() }));
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let out = autoshow::run_claude(&cmd, &prompt).and_then(|text| autoshow::parse_claude(&text, &song_copy));
            let _ = tx.send(JobResult::Claude { song, out });
        });
        self.jobs.push(Job { label: format!("Claude per «{}»", s.name), progress, rx, started: Instant::now() });
    }

    fn poll_jobs(&mut self) {
        let mut done = vec![];
        for (i, j) in self.jobs.iter().enumerate() {
            if let Ok(r) = j.rx.try_recv() {
                done.push((i, r));
            }
        }
        for (i, r) in done.into_iter().rev() {
            self.jobs.remove(i);
            match r {
                JobResult::Loaded { song, analyze, out } => match out {
                    Ok(o) => {
                        if o.audio.data.is_empty() {
                            self.status = crate::i18n::t(&format!(
                                "Nessuna traccia audio leggibile (i file sono stati spostati o rinominati?): {}",
                                o.errors.join("; ")
                            ));
                        } else if !o.errors.is_empty() {
                            self.status = format!("Alcune tracce non lette: {}", o.errors.join("; "));
                        }
                        self.waves.insert(song, o.peaks);
                        if song == self.song {
                            self.player.set_buffer(Some(o.audio));
                            self.audio_for = Some(song);
                            if let Some((from, until)) = self.pending_play.take() {
                                self.play_range(from, until);
                            }
                        }
                        if analyze {
                            if let Some(a) = o.analysis {
                                let auto_show = self.project.songs.get(song).map(|s| s.cues.is_empty()).unwrap_or(false);
                                if let Some(s) = self.project.songs.get_mut(song) {
                                    apply_analysis(s, a);
                                }
                                if auto_show {
                                    if let Some(s) = self.project.songs.get_mut(song) {
                                        let (c, h) = autoshow::generate(s);
                                        s.cues = c;
                                        s.hits = h;
                                    }
                                }
                                if o.errors.is_empty() {
                                    let s = &self.project.songs[song];
                                    let a = s.analysis.as_ref().unwrap();
                                    self.status = crate::i18n::t(&format!(
                                        "Analisi completata: {:.1} BPM, {} parti, {} stacchi/accenti — {}",
                                        a.bpm,
                                        s.sections.len(),
                                        a.accents.len() + a.stops.len(),
                                        a.character
                                    ));
                                }
                            }
                        } else if o.errors.is_empty() {
                            self.status = ts("Audio pronto per l'anteprima").into();
                        }
                    }
                    Err(e) => {
                        self.pending_play = None;
                        self.status = crate::i18n::t(&format!("Errore: {e}"));
                    }
                },
                JobResult::Claude { song, out } => match out {
                    Ok((c, h)) => {
                        self.song = song;
                        self.push_undo();
                        if let Some(s) = self.project.songs.get_mut(song) {
                            let n = c.len();
                            s.cues = c;
                            s.hits = h;
                            self.status = crate::i18n::t(&format!("Claude ha creato lo show: {n} scene. Puoi modificarlo nella scheda Show (Annulla per tornare indietro)."));
                        }
                        self.sel = Sel::None;
                    }
                    Err(e) => self.status = crate::i18n::t(&format!("Claude Code: {e}")),
                },
            }
        }
    }

    // ------------------------------------------------------------ files

    pub fn new_project(&mut self) {
        self.stop();
        self.project = Project::default();
        self.path = None;
        self.song = 0;
        self.sel = Sel::None;
        self.player.set_buffer(None);
        self.audio_for = None; self.auto_audio_tried = None; self.waves.clear();
        self.undo.clear();
        self.push_project();
        self.status = ts("Nuovo progetto: aggiungi i fari (scheda Fari e palco) e i brani (scheda Brani)").into();
    }

    pub fn load_demo(&mut self) {
        self.stop();
        let (p, a, pk) = demo_project();
        self.project = p;
        self.path = None;
        self.song = 0;
        self.sel = Sel::None;
        self.player.set_buffer(a);
        self.audio_for = Some(0);
        self.waves.clear();
        self.waves.insert(0, pk);
        self.undo.clear();
        self.push_project();
        self.status = ts("Progetto demo caricato").into();
    }

    pub fn open_dialog(&mut self) {
        if let Some(p) = rfd::FileDialog::new().add_filter(ts("Show luci"), &["luci", "json"]).pick_file() {
            match load_project(&p) {
                Ok(pr) => {
                    self.stop();
                    self.project = pr;
                    self.path = Some(p.clone());
                    self.song = 0;
                    self.sel = Sel::None;
                    self.player.set_buffer(None);
                    self.audio_for = None; self.auto_audio_tried = None; self.waves.clear();
                    self.undo.clear();
                    self.push_project();
                    self.status = crate::i18n::t(&format!("Aperto {}", p.display()));
                }
                Err(e) => self.status = e,
            }
        }
    }

    pub fn save(&mut self, as_new: bool) {
        let path = if as_new || self.path.is_none() {
            rfd::FileDialog::new()
                .add_filter(ts("Show luci"), &["luci"])
                .set_file_name(crate::i18n::t(&format!("{}.luci", self.project.name)))
                .save_file()
        } else {
            self.path.clone()
        };
        if let Some(p) = path {
            match serde_json::to_string_pretty(&self.project) {
                Ok(text) => match std::fs::write(&p, text) {
                    Ok(()) => {
                        self.status = crate::i18n::t(&format!("Salvato in {}", p.display()));
                        self.path = Some(p);
                    }
                    Err(e) => self.status = crate::i18n::t(&format!("Impossibile salvare: {e}")),
                },
                Err(e) => self.status = crate::i18n::t(&format!("Impossibile salvare: {e}")),
            }
        }
    }

    pub fn export_midi(&mut self, all: bool) {
        if all {
            let Some(dir) = rfd::FileDialog::new().pick_folder() else { return };
            let mut n = 0;
            for s in &self.project.songs {
                let (bytes, _) = crate::midi::export_song(s, &self.project.settings);
                let name: String = s.name.chars().map(|c| if c.is_alphanumeric() || c == ' ' || c == '-' { c } else { '_' }).collect();
                if std::fs::write(dir.join(format!("{:02} {} - luci.mid", s.program, name.trim())), bytes).is_ok() {
                    n += 1;
                }
            }
            self.status = crate::i18n::t(&format!("Esportati {n} file MIDI in {}", dir.display()));
        } else {
            let Some(s) = self.cur_song() else { return };
            let (bytes, list) = crate::midi::export_song(s, &self.project.settings);
            let default = crate::i18n::t(&format!("{} - luci.mid", s.name));
            if let Some(p) = rfd::FileDialog::new().add_filter(ts("MIDI"), &["mid"]).set_file_name(default).save_file() {
                match std::fs::write(&p, bytes) {
                    Ok(()) => self.status = crate::i18n::t(&format!("Traccia MIDI salvata ({} eventi) in {}", list.len(), p.display())),
                    Err(e) => self.status = crate::i18n::t(&format!("Impossibile salvare il MIDI: {e}")),
                }
            }
        }
    }

    // ------------------------------------------------------------ frame

    fn top_bar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("top").show(ctx, |ui| {
            ui.add_space(3.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new(ts("◉ LightSciò")).strong().size(17.0).color(Color32::from_rgb(255, 196, 60)));
                ui.separator();
                ui.menu_button(ts("File"), |ui| {
                    if ui.button(ts("Nuovo progetto")).clicked() {
                        self.new_project();
                        ui.close_menu();
                    }
                    if ui.button(ts("Apri…")).clicked() {
                        self.open_dialog();
                        ui.close_menu();
                    }
                    if ui.button(ts("Salva")).clicked() {
                        self.save(false);
                        ui.close_menu();
                    }
                    if ui.button(ts("Salva con nome…")).clicked() {
                        self.save(true);
                        ui.close_menu();
                    }
                    ui.separator();
                    if ui.button(ts("Esporta traccia MIDI del brano…")).clicked() {
                        self.export_midi(false);
                        ui.close_menu();
                    }
                    if ui.button(ts("Esporta MIDI di tutti i brani…")).clicked() {
                        self.export_midi(true);
                        ui.close_menu();
                    }
                    ui.separator();
                    if ui
                        .button(ts("Adatta show esistente…"))
                        .on_hover_text(ts("Importa brani e show da un altro progetto adattandoli ai fari di questo"))
                        .clicked()
                    {
                        self.open_adapt();
                        ui.close_menu();
                    }
                    if ui.button(ts("Esporta setup luci…")).on_hover_text(ts("Salva palco e fari in un file, per riusarli in altri progetti")).clicked() {
                        self.export_rig();
                        ui.close_menu();
                    }
                    if ui
                        .button(ts("Importa setup luci…"))
                        .on_hover_text(ts("Carica palco e fari da un file di setup o da un altro show"))
                        .clicked()
                    {
                        self.pick_rig_import();
                        ui.close_menu();
                    }
                    ui.separator();
                    if ui.button(ts("⚙ Preferenze…")).clicked() {
                        self.show_prefs = true;
                        ui.close_menu();
                    }
                    if ui.button(ts("⬆ Controlla aggiornamenti…")).clicked() {
                        self.show_prefs = true;
                        self.update.dismissed = false;
                        self.start_update_check(true);
                        ui.close_menu();
                    }
                    ui.separator();
                    if ui.button(ts("Carica il progetto demo")).clicked() {
                        self.load_demo();
                        ui.close_menu();
                    }
                });
                ui.separator();
                for (t, label) in [
                    (Tab::Rig, ts("🔦 Fari e palco")),
                    (Tab::Songs, ts("🎵 Brani e analisi")),
                    (Tab::Show, ts("💡 Show")),
                    (Tab::Live, ts("🔴 Live")),
                ] {
                    let sel = self.tab == t;
                    let txt = RichText::new(label).size(14.0);
                    if ui.add(egui::Button::selectable(sel, txt)).clicked() {
                        self.tab = t;
                    }
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("⚙").on_hover_text(ts("Preferenze: dimensione di testi e pulsanti")).clicked() {
                        self.show_prefs = !self.show_prefs;
                    }
                    ui.label(RichText::new(&self.project.name).color(Color32::GRAY));
                    let (live, ok) = {
                        let s = self.shared.lock();
                        match s {
                            Ok(s) => (s.dmx.port.is_some(), s.dmx_status.starts_with("Collegato")),
                            Err(_) => (false, false),
                        }
                    };
                    let (c, t) = if live && ok {
                        (Color32::from_rgb(80, 220, 120), ts("DMX attivo"))
                    } else if live {
                        (Color32::from_rgb(240, 160, 60), ts("DMX in attesa"))
                    } else {
                        (Color32::GRAY, ts("DMX spento"))
                    };
                    ui.label(RichText::new(format!("● {t}")).color(c));
                    let m = if self.midi.conn.is_some() { (ts("● MIDI in ascolto"), Color32::from_rgb(80, 220, 120)) } else { (ts("● MIDI spento"), Color32::GRAY) };
                    ui.label(RichText::new(m.0).color(m.1));
                });
            });
            ui.add_space(2.0);
        });
        egui::TopBottomPanel::bottom("status").show(ctx, |ui| {
            ui.horizontal(|ui| {
                for j in &self.jobs {
                    let p = j.progress.lock().map(|p| p.clone()).unwrap_or_default();
                    ui.spinner();
                    ui.label(crate::i18n::t(&format!("{} — {} ({:.0}s)", j.label, p.msg, j.started.elapsed().as_secs_f32())));
                    if p.frac > 0.0 {
                        ui.add(egui::ProgressBar::new(p.frac).desired_width(120.0));
                    }
                    ui.separator();
                }
                ui.label(RichText::new(t(&self.status)).color(Color32::from_rgb(190, 190, 205)));
            });
        });
    }

    fn sync_transport(&mut self) {
        let (gen, playing, by_midi, song, pos) = match self.shared.lock() {
            Ok(s) => (s.transport.gen, s.transport.playing, s.transport.by_midi, s.transport.song, s.transport.pos()),
            Err(_) => return,
        };
        if gen != self.last_gen {
            self.last_gen = gen;
            if by_midi {
                // MIDI drives the show: no preview audio
                self.player.stop();
                if song != self.song && song < self.project.songs.len() {
                    self.song = song;
                    self.sel = Sel::None;
                }
            }
        }
        if playing && !by_midi && self.player.is_playing() && self.last_realign.elapsed() > Duration::from_millis(500) {
            let ap = self.player.position();
            if let Ok(mut s) = self.shared.lock() {
                s.transport.realign(ap);
            }
            self.last_realign = Instant::now();
        }
        if !playing && self.player.is_playing() {
            self.player.stop();
        }
        if playing && !by_midi {
            let dur = self.cur_song().map(|s| s.duration()).unwrap_or(0.0);
            if pos > dur + 0.5 || self.play_until.map(|u| pos >= u).unwrap_or(false) {
                self.pause();
                self.play_until = None;
            }
        }
        if !playing {
            self.play_until = None;
        }
    }

    pub fn compute_states(&mut self) {
        let t = self.now();
        let (blackout, identify) = self
            .shared
            .lock()
            .map(|s| (s.blackout, s.identify.clone().filter(|(_, at)| at.elapsed() < Duration::from_secs(3)).map(|x| x.0)))
            .unwrap_or((false, None));
        let shown_song = self.shared.lock().map(|s| s.transport.song).unwrap_or(self.song);
        self.states = engine::render(&engine::RenderInput {
            project: &self.project,
            catalog: &self.catalog,
            song: self.project.songs.get(shown_song),
            t,
            blackout,
            identify,
        });
    }

    fn handle_screenshot(&mut self, ctx: &egui::Context) {
        let Some(shot) = &mut self.screenshot else { return };
        ctx.request_repaint();
        if self.frame >= 120 && !shot.requested {
            shot.requested = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
        }
        let mut image = None;
        ctx.input(|i| {
            for e in &i.raw.events {
                if let egui::Event::Screenshot { image: img, .. } = e {
                    image = Some(img.clone());
                }
            }
        });
        if let Some(img) = image {
            let [w, h] = img.size;
            let mut bytes = Vec::with_capacity(w * h * 4);
            for p in &img.pixels {
                bytes.extend_from_slice(&p.to_srgba_unmultiplied());
            }
            if let Some(dir) = shot.path.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            if let Err(e) = image::save_buffer(&shot.path, &bytes, w as u32, h as u32, image::ColorType::Rgba8) {
                eprintln!("screenshot: {e}");
            }
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        // safety net
        if self.frame > 1200 {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.frame += 1;
        self.poll_jobs();
        if let Some(rx) = &self.qlc_rx {
            if let Ok(list) = rx.try_recv() {
                self.qlc = list;
                self.qlc_rx = None;
            }
        }
        self.sync_transport();
        if self.last_push.elapsed() > Duration::from_millis(200) {
            self.push_project();
        }
        self.song = self.song.min(self.project.songs.len().saturating_sub(1));
        // keep the preview audio of the current song ready (e.g. after reopening a project)
        let has_tracks = self.cur_song().map(|s| !s.tracks.is_empty()).unwrap_or(false);
        if self.screenshot.is_none()
            && has_tracks
            && self.audio_for != Some(self.song)
            && self.auto_audio_tried != Some(self.song)
            && !self.job_running("Carico")
            && !self.job_running("Analisi")
        {
            self.auto_audio_tried = Some(self.song);
            self.start_load(self.song, false);
        }
        self.compute_states();
        self.top_bar(ctx);
        self.ui_update_banner(ctx);
        match self.tab {
            Tab::Rig => self.ui_rig(ctx),
            Tab::Songs => self.ui_songs(ctx),
            Tab::Show => self.ui_show(ctx),
            Tab::Live => self.ui_live(ctx),
        }
        self.ui_adapt(ctx);
        self.ui_rig_import(ctx);
        self.ui_prefs(ctx);
        self.poll_update(ctx);
        let busy = self.is_playing() || !self.jobs.is_empty() || self.midi.conn.is_some();
        if busy {
            ctx.request_repaint();
        } else {
            ctx.request_repaint_after(Duration::from_millis(250));
        }
        self.handle_screenshot(ctx);
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        if self.screenshot.is_some() {
            return;
        }
        if let Some(p) = &self.path {
            storage.set_string("last_project", p.display().to_string());
        }
        if let Ok(s) = serde_json::to_string(&self.favorites) {
            storage.set_string("favorites", s);
        }
        if let Ok(s) = serde_json::to_string(&self.qlc_dirs) {
            storage.set_string("qlc_dirs", s);
        }
        if let Ok(s) = serde_json::to_string(&self.prefs) {
            storage.set_string("prefs", s);
        }
    }
}

// ---------------------------------------------------------------- helpers

pub fn apply_analysis(s: &mut Song, a: AnalysisOut) {
    for (t, env) in s.tracks.iter_mut().zip(a.track_envs) {
        t.env = env;
    }
    s.analysis = Some(a.analysis);
    s.sections = a.sections;
    s.sort();
}

pub fn load_project(p: &std::path::Path) -> Result<Project, String> {
    let text = std::fs::read_to_string(p).map_err(|e| crate::i18n::t(&format!("Impossibile aprire {}: {e}", p.display())))?;
    let mut pr: Project = serde_json::from_str(&text).map_err(|e| crate::i18n::t(&format!("File non valido: {e}")))?;
    pr.fix();
    Ok(pr)
}

pub fn demo_project() -> (Project, Option<Arc<analysis::AudioBuf>>, Vec<Vec<f32>>) {
    let mut p = Project { name: ts("Show demo").into(), ..Project::default() };
    crate::demo::demo_rig(&mut p);
    let mut song = Song::new(ts("Brano demo (sintetico)"), 0);
    song.tracks = crate::demo::demo_tracks();
    let prog: ProgressHandle = Arc::new(Mutex::new(Progress::default()));
    let mut audio = None;
    let mut peaks = vec![];
    if let Ok(out) = analysis::load_song(&song.tracks, true, 4, &prog) {
        audio = Some(out.audio);
        peaks = out.peaks;
        if let Some(a) = out.analysis {
            apply_analysis(&mut song, a);
        }
        let (c, h) = autoshow::generate(&song);
        song.cues = c;
        song.hits = h;
    }
    p.songs.push(song);
    p.fix();
    (p, audio, peaks)
}

/// The built-in font lacks many symbols (↶ ⧉ ● ▼ …): add the system symbol
/// fonts as fallbacks so every icon is drawn instead of an empty square.
fn setup_fonts(ctx: &egui::Context) {
    let candidates = [
        r"C:\Windows\Fonts\seguisym.ttf",
        r"C:\Windows\Fonts\seguiemj.ttf",
        r"C:\Windows\Fonts\segoeui.ttf",
        "/System/Library/Fonts/Apple Symbols.ttf",
        "/System/Library/Fonts/Supplemental/Arial Unicode.ttf",
        "/Library/Fonts/Arial Unicode.ttf",
        "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
    ];
    let mut fonts = egui::FontDefinitions::default();
    let mut added = 0;
    for (i, p) in candidates.iter().enumerate() {
        if let Ok(bytes) = std::fs::read(p) {
            let name = format!("sys_symbols_{i}");
            fonts.font_data.insert(name.clone(), Arc::new(egui::FontData::from_owned(bytes)));
            for fam in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
                fonts.families.entry(fam).or_default().push(name.clone());
            }
            added += 1;
        }
    }
    if added > 0 {
        ctx.set_fonts(fonts);
    }
}

fn setup_style(ctx: &egui::Context) {
    setup_fonts(ctx);
    let mut v = egui::Visuals::dark();
    v.panel_fill = Color32::from_rgb(20, 21, 27);
    v.window_fill = Color32::from_rgb(24, 25, 32);
    v.extreme_bg_color = Color32::from_rgb(12, 12, 16);
    v.faint_bg_color = Color32::from_rgb(28, 29, 37);
    v.selection.bg_fill = Color32::from_rgb(170, 110, 20);
    v.selection.stroke = egui::Stroke::new(1.0, Color32::from_rgb(255, 210, 120));
    v.widgets.hovered.bg_fill = Color32::from_rgb(55, 57, 70);
    v.widgets.inactive.bg_fill = Color32::from_rgb(40, 42, 52);
    v.widgets.inactive.weak_bg_fill = Color32::from_rgb(40, 42, 52);
    ctx.set_visuals(v);
    apply_prefs(ctx, &Prefs::default());
}

/// Look preferences, kept across projects.
#[derive(Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Prefs {
    /// Text size multiplier.
    pub text_scale: f32,
    /// Button / control size multiplier.
    pub button_scale: f32,
    pub check_updates: bool,
    pub update_repo: String,
    pub skip_version: String,
    pub last_update_check: u64,
    /// "auto" (system), "it" or "en".
    pub language: String,
}

impl Default for Prefs {
    fn default() -> Self {
        Prefs {
            text_scale: 1.0,
            button_scale: 1.0,
            check_updates: true,
            update_repo: crate::update::DEFAULT_REPO.to_string(),
            skip_version: String::new(),
            last_update_check: 0,
            language: "auto".to_string(),
        }
    }
}

pub fn apply_prefs(ctx: &egui::Context, p: &Prefs) {
    let base = egui::Style::default();
    let t = p.text_scale.clamp(0.6, 2.5);
    let b = p.button_scale.clamp(0.6, 3.0);
    ctx.style_mut(|s| {
        for (style, font) in s.text_styles.iter_mut() {
            if let Some(def) = base.text_styles.get(style) {
                font.size = def.size * t;
            }
        }
        s.spacing.item_spacing = egui::vec2(7.0, 5.0) * b.sqrt();
        s.spacing.button_padding = egui::vec2(8.0, 3.0) * b;
        s.spacing.interact_size = egui::vec2(40.0 * b, 18.0 * b.max(t * 0.9));
        s.spacing.icon_width = 14.0 * b;
        s.spacing.icon_width_inner = 8.0 * b;
        s.spacing.slider_width = 100.0 * b;
        s.spacing.combo_height = 200.0 * b.max(1.0);
    });
}

impl App {
    pub fn ui_prefs(&mut self, ctx: &egui::Context) {
        if !self.show_prefs {
            return;
        }
        let mut open = true;
        let mut changed = false;
        let mut lang_changed = false;
        egui::Window::new(ts("⚙ Preferenze")).open(&mut open).resizable(false).default_width(380.0).show(ctx, |ui| {
            egui::Grid::new("prefs").num_columns(2).spacing([12.0, 10.0]).show(ui, |ui| {
                ui.label(ts("Dimensione testi"));
                changed |= ui.add(egui::Slider::new(&mut self.prefs.text_scale, 0.7..=2.0).custom_formatter(|v, _| format!("{:.0}%", v * 100.0))).changed();
                ui.end_row();
                ui.label(ts("Dimensione pulsanti"));
                changed |= ui.add(egui::Slider::new(&mut self.prefs.button_scale, 0.7..=2.5).custom_formatter(|v, _| format!("{:.0}%", v * 100.0))).changed();
                ui.end_row();
            });
            ui.horizontal(|ui| {
                ui.label(ts("Lingua"));
                let cur = self.prefs.language.clone();
                let name = |c: &str| match c {
                    "it" => "Italiano",
                    "en" => "English",
                    _ => ts("Automatica (sistema)"),
                };
                egui::ComboBox::from_id_salt("lang").selected_text(name(&cur)).show_ui(ui, |ui| {
                    for c in ["auto", "it", "en"] {
                        if ui.selectable_label(cur == c, name(c)).clicked() && cur != c {
                            self.prefs.language = c.to_string();
                            lang_changed = true;
                        }
                    }
                });
            });
            ui.horizontal(|ui| {
                if ui.button(ts("Ripristina")).clicked() {
                    self.prefs.text_scale = 1.0;
                    self.prefs.button_scale = 1.0;
                    changed = true;
                }
                ui.label(RichText::new(ts("Le preferenze valgono per tutti i progetti.")).size(11.0).color(Color32::GRAY));
            });
            self.ui_update_prefs(ui);
        });
        if changed {
            apply_prefs(ctx, &self.prefs);
        }
        if lang_changed {
            crate::i18n::set_english(crate::i18n::resolve(&self.prefs.language));
            // generic fixture names follow the language
            self.catalog = crate::catalog::builtin();
        }
        if !open {
            self.show_prefs = false;
        }
    }
}
