//! "Converti show per altre luci": build a new show for a different lighting
//! setup, mapping the effects of the old fixture groups onto the new ones.
//! The original show is never overwritten.

use crate::app::*;
use crate::model::*;
use egui::{Color32, RichText};

pub struct ConvertState {
    pub rig: Option<Rig>,
    pub rig_name: String,
    /// For each new group (Group::ALL order): the old group it takes the effects from.
    pub map: Vec<Option<Group>>,
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

fn load_rig(path: &std::path::Path) -> Result<Rig, String> {
    let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    if let Ok(r) = serde_json::from_str::<Rig>(&text) {
        return Ok(r);
    }
    let p: Project = serde_json::from_str(&text).map_err(|e| format!("file non valido: {e}"))?;
    Ok(Rig { stage: p.stage, custom_defs: p.custom_defs, fixtures: p.fixtures })
}

impl App {
    pub fn export_rig(&mut self) {
        let Some(p) = rfd::FileDialog::new()
            .add_filter("Setup luci", &["lucirig"])
            .set_file_name(format!("{} - setup luci.lucirig", self.project.name))
            .save_file()
        else {
            return;
        };
        let rig = Rig { stage: self.project.stage.clone(), custom_defs: self.project.custom_defs.clone(), fixtures: self.project.fixtures.clone() };
        match serde_json::to_string_pretty(&rig).map_err(|e| e.to_string()).and_then(|t| std::fs::write(&p, t).map_err(|e| e.to_string())) {
            Ok(()) => self.status = format!("Setup luci salvato in {}", p.display()),
            Err(e) => self.status = format!("Impossibile salvare il setup: {e}"),
        }
    }

    pub fn open_convert(&mut self) {
        self.convert = Some(ConvertState { rig: None, rig_name: String::new(), map: vec![None; Group::ALL.len()], error: String::new() });
    }

    fn group_summary(&self, fixtures: &[FixtureInstance], defs: &[FixtureDef], g: Group) -> (usize, String) {
        let m: Vec<&FixtureInstance> = fixtures.iter().filter(|f| f.group == g).collect();
        let types: std::collections::BTreeSet<String> = m
            .iter()
            .filter_map(|f| defs.iter().find(|d| d.id == f.def_id).or_else(|| self.catalog.iter().find(|d| d.id == f.def_id)))
            .map(|d| d.kind.label().to_string())
            .collect();
        (m.len(), types.into_iter().collect::<Vec<_>>().join(", "))
    }

    pub fn ui_convert(&mut self, ctx: &egui::Context) {
        let Some(mut st) = self.convert.take() else { return };
        let mut open = true;
        let mut create = false;
        let mut save_current = false;
        egui::Window::new("Converti show per altre luci").open(&mut open).default_width(640.0).resizable(true).show(ctx, |ui| {
            ui.label("Crea un nuovo show partendo da questo, per un setup luci diverso: brani, parti, scene e colpi restano uguali, gli effetti di ogni gruppo di fari vengono trasferiti ai gruppi del nuovo setup.");
            ui.label(RichText::new("Lo show attuale non viene modificato: il nuovo show va salvato con un altro nome.").strong());
            if ui.button("💾 Salva prima lo show attuale").on_hover_text("Consigliato se hai modifiche non salvate").clicked() {
                save_current = true;
            }
            ui.separator();
            ui.label(RichText::new("1. Scegli il nuovo setup luci").heading());
            ui.horizontal(|ui| {
                if ui.button("📂 Apri setup…").on_hover_text("Un file «setup luci» (.lucirig) o un altro show (.luci) da cui prendere palco e fari").clicked() {
                    if let Some(p) = rfd::FileDialog::new().add_filter("Setup luci o show", &["lucirig", "luci", "json"]).pick_file() {
                        match load_rig(&p) {
                            Ok(r) => {
                                let old: Vec<Group> = Group::ALL.iter().copied().filter(|g| self.project.fixtures.iter().any(|f| f.group == *g)).collect();
                                st.map = Group::ALL
                                    .iter()
                                    .map(|g| if r.fixtures.iter().any(|f| f.group == *g) { fallback(*g).iter().copied().find(|s| old.contains(s)) } else { None })
                                    .collect();
                                st.rig_name = p.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
                                st.rig = Some(r);
                                st.error.clear();
                            }
                            Err(e) => st.error = e,
                        }
                    }
                }
                if let Some(r) = &st.rig {
                    ui.label(RichText::new(format!("«{}»: {} fari", st.rig_name, r.fixtures.len())).color(Color32::from_rgb(120, 220, 140)));
                }
            });
            ui.label(RichText::new("Per creare un file di setup: prepara i fari in un progetto (anche nuovo) e usa File → Esporta setup luci.").size(11.0).color(Color32::GRAY));
            if !st.error.is_empty() {
                ui.label(RichText::new(&st.error).color(Color32::from_rgb(255, 140, 80)));
            }
            let Some(rig) = st.rig.clone() else { return };
            ui.separator();
            ui.label(RichText::new("2. Da quale gruppo prende gli effetti ogni gruppo nuovo").heading());
            let old_groups: Vec<(Group, usize, String)> = Group::ALL
                .iter()
                .map(|g| {
                    let (n, t) = self.group_summary(&self.project.fixtures, &self.project.custom_defs, *g);
                    (*g, n, t)
                })
                .filter(|x| x.1 > 0)
                .collect();
            egui::Grid::new("conv_map").num_columns(3).striped(true).spacing([12.0, 6.0]).show(ui, |ui| {
                ui.label(RichText::new("Nuovo gruppo").strong());
                ui.label(RichText::new("Fari").strong());
                ui.label(RichText::new("Prende colori, intensità, effetti e movimenti da").strong());
                ui.end_row();
                for g in Group::ALL {
                    let (n, types) = self.group_summary(&rig.fixtures, &rig.custom_defs, g);
                    if n == 0 {
                        continue;
                    }
                    ui.label(g.label());
                    ui.label(format!("{n} · {types}"));
                    let cur = &mut st.map[g.idx()];
                    let txt = match cur {
                        Some(s) => format!("{} (vecchio show)", s.label()),
                        None => "— spento —".to_string(),
                    };
                    egui::ComboBox::from_id_salt(("conv", g.idx())).width(260.0).selected_text(txt).show_ui(ui, |ui| {
                        for (og, on, ot) in &old_groups {
                            ui.selectable_value(cur, Some(*og), format!("{}  ({on} · {ot})", og.label()));
                        }
                        ui.selectable_value(cur, None, "— spento —");
                    });
                    ui.end_row();
                }
            });
            ui.label(
                RichText::new("I colpi (accecatori, strobo, flash, blackout) restano uguali e usano i gruppi Accecatori/Strobo del nuovo setup (o i PAR se mancano).")
                    .size(11.0)
                    .color(Color32::GRAY),
            );
            ui.separator();
            if ui.button(RichText::new("3. 💾 Crea e salva il nuovo show…").strong()).clicked() {
                create = true;
            }
        });
        if save_current {
            self.save(false);
        }
        if create {
            if let Some(rig) = st.rig.clone() {
                let default = format!("{} - {}.luci", self.project.name, st.rig_name);
                if let Some(p) = rfd::FileDialog::new().add_filter("Show luci", &["luci"]).set_file_name(default).save_file() {
                    if self.path.as_ref().map(|cur| cur == &p).unwrap_or(false) {
                        st.error = "Scegli un nome diverso: lo show di partenza non va sovrascritto.".into();
                    } else {
                        let mut np = self.project.clone();
                        np.stage = rig.stage.clone();
                        np.fixtures = rig.fixtures.clone();
                        np.custom_defs = rig.custom_defs.clone();
                        np.next_id = np.fixtures.iter().map(|f| f.id).max().unwrap_or(0) + 1;
                        np.name = p.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "Show convertito".into());
                        for s in &mut np.songs {
                            for c in &mut s.cues {
                                c.fix();
                                let old = c.looks.clone();
                                for g in Group::ALL {
                                    c.looks[g.idx()] = match st.map[g.idx()] {
                                        Some(src) => old[src.idx()].clone(),
                                        None => GroupLook { intensity: 0.0, ..GroupLook::default() },
                                    };
                                }
                            }
                        }
                        match serde_json::to_string_pretty(&np).map_err(|e| e.to_string()).and_then(|t| std::fs::write(&p, t).map_err(|e| e.to_string())) {
                            Ok(()) => {
                                self.project = np;
                                self.path = Some(p.clone());
                                self.undo.clear();
                                self.sel = Sel::None;
                                self.sel_set.clear();
                                self.sel_fixture = None;
                                self.push_project();
                                self.status = format!("Nuovo show creato e aperto: {} (l'originale non è stato toccato)", p.display());
                                return;
                            }
                            Err(e) => st.error = format!("Impossibile salvare: {e}"),
                        }
                    }
                }
            }
        }
        if open {
            self.convert = Some(st);
        }
    }
}
