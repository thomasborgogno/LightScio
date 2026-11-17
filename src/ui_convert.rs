//! "Adatta show esistente": import the songs and the show of another project
//! into this one (which has a different lighting rig), mapping the effects of
//! the old fixture groups onto the groups of this rig. The other file is
//! never modified.

use crate::app::*;
use crate::i18n::{t, ts};
use crate::model::*;
use egui::{Color32, RichText};

pub struct AdaptState {
    pub source: Option<Project>,
    pub source_name: String,
    /// Which songs of the source to import.
    pub songs: Vec<bool>,
    /// For each group of THIS rig (Group::ALL order): the source group it takes the effects from.
    pub map: Vec<Option<Group>>,
    pub replace_same_name: bool,
    pub error: String,
}

fn fallback(g: Group) -> &'static [Group] {
    match g {
        Group::FrontWash => &[Group::FrontWash, Group::BackWash, Group::Bars, Group::Extra],
        Group::BackWash => &[Group::BackWash, Group::FrontWash, Group::Bars, Group::Extra],
        Group::Blinders => &[Group::Blinders],
        Group::HeadsFront => &[Group::HeadsFront, Group::HeadsBack],
        Group::HeadsBack => &[Group::HeadsBack, Group::HeadsFront],
        Group::Strobes => &[Group::Strobes],
        Group::Bars => &[Group::Bars, Group::BackWash, Group::FrontWash],
        Group::Extra => &[Group::Extra, Group::BackWash, Group::FrontWash],
    }
}

impl App {
    /// Save stage + fixtures (+ custom fixture models) to a .lucirig file.
    pub fn export_rig(&mut self) {
        let Some(p) = rfd::FileDialog::new()
            .add_filter(ts("Setup luci"), &["lucirig"])
            .set_file_name(format!("{} - setup.lucirig", self.project.name))
            .save_file()
        else {
            return;
        };
        let rig = Rig { stage: self.project.stage.clone(), custom_defs: self.project.custom_defs.clone(), fixtures: self.project.fixtures.clone() };
        match serde_json::to_string_pretty(&rig).map_err(|e| e.to_string()).and_then(|txt| std::fs::write(&p, txt).map_err(|e| e.to_string())) {
            Ok(()) => self.status = t(&format!("Setup luci salvato in {}", p.display())),
            Err(e) => self.status = t(&format!("Impossibile salvare il setup: {}", e)),
        }
    }

    /// Choose a .lucirig (or a .luci show, to take its lights) to import.
    pub fn pick_rig_import(&mut self) {
        let Some(p) = rfd::FileDialog::new().add_filter(ts("Setup luci o show"), &["lucirig", "luci", "json"]).pick_file() else { return };
        let rig = std::fs::read_to_string(&p).map_err(|e| e.to_string()).and_then(|text| {
            serde_json::from_str::<Rig>(&text).or_else(|_| {
                serde_json::from_str::<Project>(&text)
                    .map(|pr| Rig { stage: pr.stage, custom_defs: pr.custom_defs, fixtures: pr.fixtures })
                    .map_err(|e| e.to_string())
            })
        });
        match rig {
            Ok(r) => {
                let name = p.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
                if self.project.fixtures.is_empty() {
                    self.apply_rig(r, true, &name);
                } else {
                    self.rig_import = Some((r, name));
                }
            }
            Err(e) => self.status = t(&format!("File non valido: {}", e)),
        }
    }

    fn apply_rig(&mut self, r: Rig, replace: bool, name: &str) {
        for d in r.custom_defs {
            if let Some(x) = self.project.custom_defs.iter_mut().find(|x| x.id == d.id) {
                *x = d;
            } else {
                self.project.custom_defs.push(d);
            }
        }
        let n = r.fixtures.len();
        if replace {
            self.project.stage = r.stage;
            self.project.fixtures = r.fixtures;
        } else {
            // add after the current ones: new ids, DMX addresses after the last used one
            let addr = self.project.next_address(&self.catalog);
            let first = r.fixtures.iter().map(|f| f.address).min().unwrap_or(1);
            for mut f in r.fixtures {
                f.id = self.project.next_id;
                self.project.next_id += 1;
                f.address = (addr + (f.address - first)).min(512);
                self.project.fixtures.push(f);
            }
        }
        self.project.fix();
        self.sel_set.clear();
        self.sel_fixture = None;
        self.push_project();
        self.status = t(&format!("Importati {} fari dal setup «{}»", n, name));
    }

    /// Ask whether to replace the current lights or add the imported ones.
    pub fn ui_rig_import(&mut self, ctx: &egui::Context) {
        let Some((rig, name)) = self.rig_import.take() else { return };
        let mut choice: Option<bool> = None;
        let mut cancel = false;
        egui::Window::new(ts("Importa setup luci")).collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
            ui.label(t(&format!("Il setup «{}» contiene {} fari. Questo progetto ne ha già {}.", name, rig.fixtures.len(), self.project.fixtures.len())));
            ui.horizontal(|ui| {
                if ui.button(RichText::new(ts("Sostituisci i fari attuali")).strong()).clicked() {
                    choice = Some(true);
                }
                if ui.button(ts("Aggiungi a quelli attuali")).clicked() {
                    choice = Some(false);
                }
                if ui.button(ts("Annulla")).clicked() {
                    cancel = true;
                }
            });
            ui.label(RichText::new(ts("Sostituendo, anche le misure del palco vengono prese dal setup.")).size(11.0).color(Color32::GRAY));
        });
        match choice {
            Some(replace) => self.apply_rig(rig, replace, &name),
            None if !cancel => self.rig_import = Some((rig, name)),
            None => {}
        }
    }

    pub fn open_adapt(&mut self) {
        self.adapt = Some(AdaptState {
            source: None,
            source_name: String::new(),
            songs: vec![],
            map: vec![None; Group::ALL.len()],
            replace_same_name: true,
            error: String::new(),
        });
    }

    fn group_summary(&self, p: &Project, g: Group) -> (usize, String) {
        let m: Vec<&FixtureInstance> = p.fixtures.iter().filter(|f| f.group == g).collect();
        let types: std::collections::BTreeSet<String> =
            m.iter().filter_map(|f| p.def(&self.catalog, &f.def_id)).map(|d| d.kind.label().to_string()).collect();
        (m.len(), types.into_iter().collect::<Vec<_>>().join(", "))
    }

    pub fn ui_adapt(&mut self, ctx: &egui::Context) {
        let Some(mut st) = self.adapt.take() else { return };
        let mut open = true;
        let mut import = false;
        egui::Window::new(ts("Adatta show esistente")).open(&mut open).default_width(660.0).resizable(true).show(ctx, |ui| {
            ui.label(ts("Importa i brani e lo show di un altro progetto adattandoli ai fari di questo progetto: brani, parti, scene e colpi restano uguali, gli effetti di ogni gruppo di fari vengono trasferiti ai gruppi di questo setup."));
            ui.label(RichText::new(ts("Il file da cui importi non viene modificato.")).strong());
            if self.project.fixtures.is_empty() {
                ui.label(RichText::new(ts("Questo progetto non ha ancora fari: aggiungili prima in «Fari e palco».")).color(Color32::from_rgb(255, 150, 80)));
                return;
            }
            ui.separator();
            ui.label(RichText::new(ts("1. Scegli lo show da adattare")).heading());
            ui.horizontal(|ui| {
                if ui.button(ts("📂 Apri show…")).clicked() {
                    if let Some(p) = rfd::FileDialog::new().add_filter(ts("Show luci"), &["luci", "json"]).pick_file() {
                        match load_project(&p) {
                            Ok(src) => {
                                let src_groups: Vec<Group> = Group::ALL.iter().copied().filter(|g| src.fixtures.iter().any(|f| f.group == *g)).collect();
                                st.map = Group::ALL
                                    .iter()
                                    .map(|g| {
                                        if self.project.fixtures.iter().any(|f| f.group == *g) {
                                            fallback(*g).iter().copied().find(|s| src_groups.contains(s))
                                        } else {
                                            None
                                        }
                                    })
                                    .collect();
                                st.songs = vec![true; src.songs.len()];
                                st.source_name = p.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
                                st.source = Some(src);
                                st.error.clear();
                            }
                            Err(e) => st.error = e,
                        }
                    }
                }
                if let Some(src) = &st.source {
                    ui.label(RichText::new(t(&format!("«{}»: {} brani, {} fari", st.source_name, src.songs.len(), src.fixtures.len()))).color(Color32::from_rgb(120, 220, 140)));
                }
            });
            if !st.error.is_empty() {
                ui.label(RichText::new(t(&st.error)).color(Color32::from_rgb(255, 140, 80)));
            }
            let Some(src) = st.source.clone() else { return };
            ui.separator();
            ui.label(RichText::new(ts("2. Brani da importare")).heading());
            egui::ScrollArea::vertical().id_salt("adapt_songs").max_height(150.0).show(ui, |ui| {
                for (i, s) in src.songs.iter().enumerate() {
                    let exists = self.project.songs.iter().any(|x| x.name == s.name);
                    let mut label = format!("{:>3}  {}  ({} {})", s.program, s.name, s.cues.len(), ts("scene"));
                    if exists {
                        label.push_str(&format!("  — {}", ts("già presente")));
                    }
                    ui.checkbox(&mut st.songs[i], label);
                }
            });
            ui.horizontal(|ui| {
                if ui.small_button(ts("Tutti")).clicked() {
                    st.songs.iter_mut().for_each(|x| *x = true);
                }
                if ui.small_button(ts("Nessuno")).clicked() {
                    st.songs.iter_mut().for_each(|x| *x = false);
                }
                ui.checkbox(&mut st.replace_same_name, ts("Sostituisci i brani con lo stesso nome"));
            });
            ui.separator();
            ui.label(RichText::new(ts("3. Da quale gruppo dello show prende gli effetti ogni gruppo di questo progetto")).heading());
            let src_groups: Vec<(Group, usize, String)> = Group::ALL
                .iter()
                .map(|g| {
                    let (n, t) = self.group_summary(&src, *g);
                    (*g, n, t)
                })
                .filter(|x| x.1 > 0)
                .collect();
            egui::Grid::new("adapt_map").num_columns(3).striped(true).spacing([12.0, 6.0]).show(ui, |ui| {
                ui.label(RichText::new(ts("Gruppo di questo progetto")).strong());
                ui.label(RichText::new(ts("Fari")).strong());
                ui.label(RichText::new(ts("Prende colori, intensità, effetti e movimenti da")).strong());
                ui.end_row();
                for g in Group::ALL {
                    let (n, types) = self.group_summary(&self.project, g);
                    if n == 0 {
                        continue;
                    }
                    ui.label(g.label());
                    ui.label(format!("{n} · {types}"));
                    let cur = &mut st.map[g.idx()];
                    let txt = match cur {
                        Some(s) => s.label().to_string(),
                        None => ts("— spento —").to_string(),
                    };
                    egui::ComboBox::from_id_salt(("adapt", g.idx())).width(280.0).selected_text(txt).show_ui(ui, |ui| {
                        for (og, on, ot) in &src_groups {
                            ui.selectable_value(cur, Some(*og), format!("{}  ({on} · {ot})", og.label()));
                        }
                        ui.selectable_value(cur, None, ts("— spento —"));
                    });
                    ui.end_row();
                }
            });
            ui.label(
                RichText::new(ts("I colpi (accecatori, strobo, flash, blackout) restano uguali e usano i gruppi Accecatori/Strobo di questo progetto (o i PAR se mancano)."))
                    .size(11.0)
                    .color(Color32::GRAY),
            );
            ui.separator();
            let n = st.songs.iter().filter(|x| **x).count();
            if ui.add_enabled(n > 0, egui::Button::new(RichText::new(t(&format!("Importa {} brani nel progetto", n))).strong())).clicked() {
                import = true;
            }
        });
        if import {
            if let Some(src) = st.source.clone() {
                let mut added = 0;
                for (i, s) in src.songs.iter().enumerate() {
                    if !st.songs.get(i).copied().unwrap_or(false) {
                        continue;
                    }
                    let mut song = s.clone();
                    for c in &mut song.cues {
                        c.fix();
                        let old = c.looks.clone();
                        for g in Group::ALL {
                            c.looks[g.idx()] = match st.map[g.idx()] {
                                Some(srcg) => old[srcg.idx()].clone(),
                                None => GroupLook { intensity: 0.0, ..GroupLook::default() },
                            };
                        }
                    }
                    if let Some(j) = self.project.songs.iter().position(|x| x.name == song.name).filter(|_| st.replace_same_name) {
                        song.program = self.project.songs[j].program;
                        self.project.songs[j] = song;
                    } else {
                        if self.project.songs.iter().any(|x| x.program == song.program) {
                            song.program = (0..128u8).find(|p| !self.project.songs.iter().any(|x| x.program == *p)).unwrap_or(song.program);
                        }
                        self.project.songs.push(song);
                    }
                    added += 1;
                }
                self.project.fix();
                self.audio_for = None;
                self.auto_audio_tried = None;
                self.waves.clear();
                self.undo.clear();
                self.sel = Sel::None;
                self.push_project();
                self.status = t(&format!("Importati {} brani adattati a questo setup luci. Ricordati di salvare il progetto.", added));
                return;
            }
        }
        if open {
            self.adapt = Some(st);
        }
    }
}
