//! "Brani e analisi" tab: songs, multitrack import and analysis results.

use crate::app::*;
#[allow(unused_imports)]
use crate::i18n::{t, ts};
use crate::model::*;
use egui::{Color32, RichText};

pub enum SecAct {
    Play(f64, f64),
    Merge(usize),
    SplitAt(f64),
}

impl App {
    pub fn section_action(&mut self, a: SecAct) {
        match a {
            SecAct::Play(from, until) => self.play_range(from, Some(until)),
            other => {
                self.push_undo();
                let Some(s) = self.cur_song_mut() else { return };
                let Some(an) = s.analysis.clone() else { return };
                let bar = an.bar_len().max(0.1);
                let snap_bar = |t: f64| an.snap_bar(t);
                match other {
                    SecAct::Merge(i) => {
                        if i + 1 < s.sections.len() {
                            let end = s.sections[i + 1].end;
                            s.sections[i].end = end;
                            s.sections.remove(i + 1);
                        }
                    }
                    SecAct::SplitAt(t) => {
                        let t = snap_bar(t);
                        if let Some(i) = s.section_at(t) {
                            let sec = s.sections[i].clone();
                            if t - sec.start >= bar * 0.99 && sec.end - t >= bar * 0.99 {
                                s.sections[i].end = t;
                                let mut n = sec.clone();
                                n.start = t;
                                n.label = crate::i18n::t(&format!("{} (b)", sec.label));
                                s.sections.insert(i + 1, n);
                            }
                        }
                    }
                    _ => {}
                }
                self.status = ts("Parti aggiornate (Annulla nella scheda Show per tornare indietro). Rigenera lo show per adattare le luci.").into();
            }
        }
    }

    fn pick_tracks() -> Vec<TrackInfo> {
        let files = rfd::FileDialog::new()
            .set_title(ts("Scegli le tracce separate del brano (MP3, WAV, FLAC, OGG)"))
            .add_filter(ts("Audio"), &["mp3", "wav", "flac", "ogg"])
            .pick_files()
            .unwrap_or_default();
        files
            .into_iter()
            .map(|p| {
                let name = p.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
                TrackInfo { path: p.display().to_string(), role: TrackRole::guess(&name), name, env: vec![], hidden: false }
            })
            .collect()
    }

    pub fn ui_songs(&mut self, ctx: &egui::Context) {
        egui::SidePanel::left("songs").default_width(260.0).resizable(true).show(ctx, |ui| {
            ui.heading(ts("Scaletta"));
            ui.label(RichText::new(ts("Il numero è il Program Change MIDI che seleziona il brano dal vivo.")).size(11.0).color(Color32::GRAY));
            let mut pick = None;
            let mut mv: Option<(usize, usize)> = None;
            let n = self.project.songs.len();
            for (i, s) in self.project.songs.iter().enumerate() {
                ui.horizontal(|ui| {
                    let state = if s.analysis.is_some() { "✔" } else { "…" };
                    if ui.selectable_label(i == self.song, format!("{:>3}  {}  {}", s.program, s.name, state)).clicked() {
                        pick = Some(i);
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if i + 1 < n && ui.small_button("↓").clicked() {
                            mv = Some((i, i + 1));
                        }
                        if i > 0 && ui.small_button("↑").clicked() {
                            mv = Some((i, i - 1));
                        }
                    });
                });
            }
            if let Some((a, b)) = mv {
                self.project.songs.swap(a, b);
                if self.song == a {
                    self.song = b;
                } else if self.song == b {
                    self.song = a;
                }
                self.audio_for = None; self.auto_audio_tried = None; self.waves.clear();
                self.player.set_buffer(None);
            }
            if let Some(i) = pick {
                self.select_song(i);
            }
            ui.separator();
            if ui.button(RichText::new(ts("➕ Nuovo brano dalle tracce…")).strong()).clicked() {
                let tracks = Self::pick_tracks();
                if !tracks.is_empty() {
                    let prog = (0..128u8).find(|p| !self.project.songs.iter().any(|s| s.program == *p)).unwrap_or(0);
                    let name = std::path::Path::new(&tracks[0].path)
                        .parent()
                        .and_then(|p| p.file_name())
                        .map(|s| s.to_string_lossy().to_string())
                        .unwrap_or_else(|| crate::i18n::t(&format!("Brano {}", self.project.songs.len() + 1)));
                    let mut s = Song::new(&name, prog);
                    s.tracks = tracks;
                    self.project.songs.push(s);
                    let i = self.project.songs.len() - 1;
                    self.select_song(i);
                    self.start_load(i, true);
                }
            }
            if ui.button(ts("➕ Brano vuoto")).clicked() {
                let prog = (0..128u8).find(|p| !self.project.songs.iter().any(|s| s.program == *p)).unwrap_or(0);
                self.project.songs.push(Song::new(&crate::i18n::t(&format!("Brano {}", self.project.songs.len() + 1)), prog));
                let i = self.project.songs.len() - 1;
                self.select_song(i);
            }
            if !self.project.songs.is_empty() && ui.button(ts("🗑 Elimina il brano selezionato")).clicked() {
                let i = self.song;
                self.stop();
                self.project.songs.remove(i);
                self.undo.clear();
                self.audio_for = None; self.auto_audio_tried = None; self.waves.clear();
                self.player.set_buffer(None);
                self.song = 0;
                self.sel = Sel::None;
            }
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            let busy = self.job_running("Analisi") || self.job_running("Carico");
            let si = self.song;
            let Some(song) = self.project.songs.get_mut(si) else {
                ui.centered_and_justified(|ui| {
                    ui.label(RichText::new(ts("Aggiungi un brano: scegli tutte le tracce separate (batteria, basso, chitarre, tastiere, voci, click…) della stessa canzone.")).size(15.0));
                });
                return;
            };
            let mut analyze = false;
            let mut add = false;
            let mut relink = false;
            let mut gen_rules = false;
            let mut gen_claude = false;
            ui.horizontal(|ui| {
                ui.label(ts("Titolo"));
                ui.add(egui::TextEdit::singleline(&mut song.name).desired_width(240.0));
                ui.label(ts("Program Change"));
                ui.add(egui::DragValue::new(&mut song.program).range(0..=127));
                ui.separator();
                if ui.button(ts("➕ Aggiungi tracce…")).clicked() {
                    add = true;
                }
                ui.label(ts("Battiti per battuta"));
                ui.add(egui::DragValue::new(&mut self.bpb_for_analysis).range(2..=12));
                if ui
                    .add_enabled(!busy && !song.tracks.is_empty(), egui::Button::new(RichText::new(ts("🔍 Analizza il brano")).strong()))
                    .on_hover_text(ts("Riconosce tempo, battute, parti (intro, strofa, ritornello…), intensità, colpi di batteria e stacchi"))
                    .clicked()
                {
                    analyze = true;
                }
                if busy {
                    ui.spinner();
                }
                ui.separator();
                let has = song.analysis.is_some();
                if ui.add_enabled(has, egui::Button::new(ts("✨ Genera lo show (regole)"))).clicked() {
                    gen_rules = true;
                }
                if ui
                    .add_enabled(has, egui::Button::new(ts("🤖 Genera con Claude Code")))
                    .on_hover_text(ts("Usa Claude Code installato sul computer per progettare lo show"))
                    .clicked()
                {
                    gen_claude = true;
                }
                ui.label(RichText::new(crate::i18n::t(&format!("{} scene · {} colpi", song.cues.len(), song.hits.len()))).color(Color32::GRAY));
            });
            egui::CollapsingHeader::new(t(&format!("Tracce ({})", song.tracks.len()))).id_salt("tracks_box").default_open(song.analysis.is_none()).show(ui, |ui| {
                ui.label(
                    RichText::new(ts("Indica cosa suona ogni traccia: la batteria guida colpi e stacchi, il click (se c'è) dà il tempo esatto ed è escluso dall'ascolto. L'occhio mostra/nasconde la traccia negli editor."))
                        .size(11.0)
                        .color(Color32::GRAY),
                );
                let mut remove = None;
                egui::Grid::new("tracks").num_columns(5).striped(true).show(ui, |ui| {
                    for (i, t) in song.tracks.iter_mut().enumerate() {
                        let mut vis = !t.hidden;
                        if ui.checkbox(&mut vis, "👁").on_hover_text(ts("Mostra la traccia negli editor")).changed() {
                            t.hidden = !vis;
                        }
                        ui.label(RichText::new(&t.name).color(crate::lanes::role_color(t.role)));
                        egui::ComboBox::from_id_salt(("role", i)).selected_text(t.role.label()).show_ui(ui, |ui| {
                            for r in TrackRole::ALL {
                                ui.selectable_value(&mut t.role, r, r.label());
                            }
                        });
                        let p: String = if t.path.starts_with("demo:") { "(traccia sintetica della demo)".into() } else { t.path.clone() };
                        let ok = crate::relink::track_available(&t.path);
                        let l = ui.label(RichText::new(p).size(10.5).color(if ok { Color32::GRAY } else { Color32::from_rgb(230, 110, 90) }));
                        if !ok {
                            l.on_hover_text(ts("File non trovato su questo computer: usa «Ricollega tracce»"));
                        }
                        if ui.small_button("×").on_hover_text(ts("Togli la traccia")).clicked() {
                            remove = Some(i);
                        }
                        ui.end_row();
                    }
                });
                if let Some(i) = remove {
                    song.tracks.remove(i);
                }
                if ui
                    .button(ts("🔗 Ricollega tracce…"))
                    .on_hover_text(ts("Indica la nuova cartella degli MP3 dopo aver spostato il progetto su un altro computer"))
                    .clicked()
                {
                    relink = true;
                }
                ui.horizontal(|ui| {
                    ui.label(ts("Comando di Claude Code"));
                    ui.add(egui::TextEdit::singleline(&mut self.project.settings.claude_command).desired_width(200.0));
                });
            });
            ui.separator();
            if self.project.songs[si].analysis.is_some() {
                self.analysis_editor(ui);
            } else {
                ui.label(ts("Il brano non è ancora stato analizzato: premi «Analizza il brano»."));
            }
            if add {
                let t = Self::pick_tracks();
                if let Some(s) = self.project.songs.get_mut(si) {
                    s.tracks.extend(t);
                }
            }
            // keyboard: Space = play/pause, Ctrl+Z = undo
            if ui.ctx().memory(|m| m.focused().is_none()) {
                let (space, undo) = ui.ctx().input(|i| (i.key_pressed(egui::Key::Space), i.modifiers.command && i.key_pressed(egui::Key::Z)));
                if space {
                    if self.is_playing() {
                        self.pause()
                    } else {
                        self.play()
                    }
                }
                if undo {
                    self.do_undo();
                    self.ed.sel = None;
                }
            }
            if relink {
                self.relink_dialog();
            }
            if analyze {
                self.start_load(si, true);
            }
            if gen_rules {
                self.regenerate_rules();
            }
            if gen_claude {
                self.start_claude(si);
            }
        });
    }
}