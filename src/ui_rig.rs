//! "Fari e palco" tab: fixture catalog (generic, favourites, Open Fixture Library),
//! custom fixtures, patch, placement and multi-selection editing.

use crate::app::*;
#[allow(unused_imports)]
use crate::i18n::{t, ts};
use crate::model::*;
use crate::stage;
use egui::{vec2, Color32, RichText};
use std::time::Instant;

const MAX_RESULTS: usize = 200;

impl App {
    fn all_defs(&self) -> Vec<FixtureDef> {
        let mut v = self.project.custom_defs.clone();
        for f in &self.favorites {
            if !v.iter().any(|d| d.id == f.id) {
                v.push(f.clone());
            }
        }
        v.extend(self.catalog.iter().cloned());
        v
    }

    /// The fixture model currently picked in the catalog (generic, favourite or library).
    fn picked_def(&mut self) -> Option<FixtureDef> {
        if let Some((path, mode)) = self.ofl_pick.clone() {
            let entry = self.ofl_cache.entry(path.clone()).or_insert_with(|| crate::ofl::load(&path));
            return entry.as_ref().ok().and_then(|v| v.get(mode).or(v.first()).cloned());
        }
        self.all_defs().into_iter().find(|d| d.id == self.catalog_pick)
    }

    fn select_fixture(&mut self, id: u32, additive: bool) {
        if additive {
            if !self.sel_set.remove(&id) {
                self.sel_set.insert(id);
                self.sel_fixture = Some(id);
            } else if self.sel_fixture == Some(id) {
                self.sel_fixture = self.sel_set.iter().next().copied();
            }
        } else {
            self.sel_set.clear();
            self.sel_set.insert(id);
            self.sel_fixture = Some(id);
        }
    }

    fn place_new(&self, kind: FixtureKind, k: usize, n: usize) -> (f32, f32, f32, bool, f32) {
        let st = &self.project.stage;
        let span = st.width * 0.8;
        let x = -span / 2.0 + (k as f32 + 0.5) * span / n.max(1) as f32;
        match kind {
            FixtureKind::Par => (x, st.depth - 0.4, st.truss_back, true, 20.0),
            FixtureKind::Blinder => (x, st.depth - 0.3, 0.4, false, 75.0),
            FixtureKind::Strobe => (x * 0.3, st.depth - 0.5, st.truss_back, true, 30.0),
            FixtureKind::LedBar => (x, st.depth - 0.2, 0.1, false, 80.0),
            k if k.is_moving() => (x, 1.0, 0.3, false, 0.0),
            _ => (x, 0.4, st.truss_front, true, -35.0),
        }
    }

    fn add_fixtures(&mut self, def: FixtureDef, count: u32) {
        // models coming from favourites / the library are copied into the project
        if !self.catalog.iter().any(|d| d.id == def.id) {
            if let Some(x) = self.project.custom_defs.iter_mut().find(|d| d.id == def.id) {
                *x = def.clone();
            } else {
                self.project.custom_defs.push(def.clone());
            }
        }
        let n = count.max(1) as usize;
        let mut addr = self.project.next_address(&self.catalog);
        let existing = self.project.fixtures.iter().filter(|f| f.def_id == def.id).count();
        self.sel_set.clear();
        for k in 0..n {
            let (x, y, z, hanging, aim) = self.place_new(def.kind, k, n);
            let id = self.project.next_id;
            self.project.next_id += 1;
            let group = auto_group(def.kind, y, self.project.stage.depth);
            let short = def.kind.label().split_whitespace().next().unwrap_or(ts("Faro")).to_string();
            self.project.fixtures.push(FixtureInstance {
                id,
                def_id: def.id.clone(),
                name: format!("{} {}", short, existing + k + 1),
                address: addr.min(512),
                x,
                y,
                z,
                hanging,
                group,
                aim_pan: 0.0,
                aim_tilt: aim,
            });
            self.sel_set.insert(id);
            self.sel_fixture = Some(id);
            addr = addr.saturating_add(def.footprint());
        }
        self.status = crate::i18n::t(&format!("Aggiunti {n} × {}", def.name));
    }

    fn conflicts(&self) -> Vec<String> {
        let mut out = vec![];
        let ranges: Vec<(u32, u16, u16, String)> = self
            .project
            .fixtures
            .iter()
            .map(|f| {
                let n = self.project.def(&self.catalog, &f.def_id).map(|d| d.footprint()).unwrap_or(1);
                (f.id, f.address, f.address + n - 1, f.name.clone())
            })
            .collect();
        for (i, a) in ranges.iter().enumerate() {
            if a.2 > 512 || a.1 == 0 {
                out.push(crate::i18n::t(&format!("{} esce dall'universo DMX (1-512)", a.3)));
            }
            for b in ranges.iter().skip(i + 1) {
                if a.1 <= b.2 && b.1 <= a.2 {
                    out.push(crate::i18n::t(&format!("{} e {} si sovrappongono", a.3, b.3)));
                }
            }
        }
        out
    }

    fn catalog_panel(&mut self, ui: &mut egui::Ui) {
        ui.heading(ts("Catalogo fari"));
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut self.search).hint_text(ts("🔍 Cerca marca o modello…")).desired_width(260.0));
            if !self.search.is_empty() && ui.small_button("×").clicked() {
                self.search.clear();
            }
        });
        let q = self.search.trim().to_lowercase();
        let words: Vec<&str> = q.split_whitespace().collect();
        let matches = |text: &str| {
            let t = text.to_lowercase();
            words.iter().all(|w| t.contains(w))
        };
        let defs = self.all_defs();
        let mut pick_def: Option<String> = None;
        let mut pick_ofl: Option<std::path::PathBuf> = None;
        egui::ScrollArea::vertical().max_height(ui.available_height() * 0.5).id_salt("cat_list").auto_shrink([false, false]).show(ui, |ui| {
            let fav_ids: Vec<String> = self.favorites.iter().map(|f| f.id.clone()).collect();
            let favs: Vec<&FixtureDef> = self.favorites.iter().filter(|d| matches(&d.name)).collect();
            if !favs.is_empty() {
                ui.label(RichText::new(ts("★ Preferiti")).strong().color(Color32::from_rgb(255, 210, 80)));
                for d in favs {
                    let sel = self.ofl_pick.is_none() && self.catalog_pick == d.id;
                    if ui.selectable_label(sel, crate::i18n::t(&format!("★ {} · {} can.", d.name, d.channels.len()))).clicked() {
                        pick_def = Some(d.id.clone());
                    }
                }
            }
            let mine: Vec<&FixtureDef> =
                self.project.custom_defs.iter().filter(|d| !fav_ids.contains(&d.id) && matches(&d.name)).collect();
            if !mine.is_empty() {
                ui.label(RichText::new(ts("Nel progetto")).strong().color(Color32::from_rgb(255, 200, 100)));
                for d in mine {
                    let sel = self.ofl_pick.is_none() && self.catalog_pick == d.id;
                    if ui.selectable_label(sel, crate::i18n::t(&format!("{} · {} can.", d.name, d.channels.len()))).clicked() {
                        pick_def = Some(d.id.clone());
                    }
                }
            }
            for kind in FixtureKind::ALL {
                let items: Vec<&FixtureDef> =
                    self.catalog.iter().filter(|d| d.kind == kind && (matches(&d.name) || matches(kind.label()))).collect();
                if items.is_empty() {
                    continue;
                }
                ui.label(RichText::new(crate::i18n::t(&format!("Generici · {}", kind.label()))).strong().color(Color32::from_rgb(200, 170, 110)));
                for d in items {
                    let sel = self.ofl_pick.is_none() && self.catalog_pick == d.id;
                    if ui.selectable_label(sel, crate::i18n::t(&format!("{} · {} can.", d.name, d.channels.len()))).clicked() {
                        pick_def = Some(d.id.clone());
                    }
                }
            }
            ui.separator();
            if self.ofl.is_empty() {
                if let Some(b) = self.ofl_busy {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(ts(b));
                    });
                } else {
                    ui.label(RichText::new(ts("Libreria fixture non ancora scaricata: vedi «Libreria fixture» qui sotto.")).color(Color32::GRAY));
                }
            } else if words.is_empty() {
                ui.label(RichText::new(crate::i18n::t(&format!("Libreria fixture: {} modelli. Scrivi nella ricerca per trovarli.", self.ofl.len()))).color(Color32::GRAY));
            } else {
                let found: Vec<&crate::ofl::OflEntry> = self.ofl.iter().filter(|e| words.iter().all(|w| e.key.contains(w))).collect();
                ui.label(RichText::new(crate::i18n::t(&format!("Libreria fixture · {} risultati", found.len()))).strong().color(Color32::from_rgb(120, 190, 255)));
                for e in found.iter().take(MAX_RESULTS) {
                    let sel = self.ofl_pick.as_ref().map(|(p, _)| p == &e.path).unwrap_or(false);
                    if ui.selectable_label(sel, format!("{} — {}  ({})", e.manufacturer, e.model, e.kind)).clicked() {
                        pick_ofl = Some(e.path.clone());
                    }
                }
                if found.len() > MAX_RESULTS {
                    ui.label(RichText::new(crate::i18n::t(&format!("…e altri {}: affina la ricerca", found.len() - MAX_RESULTS))).color(Color32::GRAY));
                }
            }
        });
        if let Some(id) = pick_def {
            self.catalog_pick = id;
            self.ofl_pick = None;
        }
        if let Some(p) = pick_ofl {
            self.ofl_pick = Some((p, 0));
        }
        ui.separator();

        // details of the picked model
        if let Some((path, mode)) = self.ofl_pick.clone() {
            let res = self.ofl_cache.entry(path.clone()).or_insert_with(|| crate::ofl::load(&path)).clone();
            match res {
                Ok(modes) if modes.len() > 1 => {
                    let mut m = mode.min(modes.len() - 1);
                    egui::ComboBox::from_id_salt("ofl_mode")
                        .width(280.0)
                        .selected_text(crate::i18n::t(&format!("Modo: {} ({} can.)", modes[m].name.rsplit("— ").next().unwrap_or(""), modes[m].channels.len())))
                        .show_ui(ui, |ui| {
                            for (i, d) in modes.iter().enumerate() {
                                ui.selectable_value(&mut m, i, crate::i18n::t(&format!("{} ({} canali)", d.name.rsplit("— ").next().unwrap_or(""), d.channels.len())));
                            }
                        });
                    self.ofl_pick = Some((path, m));
                }
                Ok(_) => {}
                Err(e) => {
                    ui.label(RichText::new(crate::i18n::t(&format!("Impossibile leggere il file: {e}"))).color(Color32::from_rgb(255, 140, 80)));
                }
            }
        }
        if let Some(d) = self.picked_def() {
            ui.label(RichText::new(&d.name).strong());
            ui.label(RichText::new(format!("{} · {}", d.kind.label(), d.capabilities())).size(11.0));
            egui::ScrollArea::vertical().max_height(200.0).id_salt("ch_list").show(ui, |ui| crate::ui_fixture_info::channels_ui(ui, "cat_ch", &d));
            ui.horizontal(|ui| {
                ui.label(ts("Quanti"));
                ui.add(egui::DragValue::new(&mut self.add_count).range(1..=24));
                if ui.button(RichText::new(ts("➕ Aggiungi al palco")).strong()).clicked() {
                    self.add_fixtures(d.clone(), self.add_count);
                }
            });
            ui.horizontal(|ui| {
                let is_fav = self.favorites.iter().any(|f| f.id == d.id);
                let star = if is_fav { ts("★ Togli dai preferiti") } else { ts("☆ Aggiungi ai preferiti") };
                if ui.button(star).clicked() {
                    if is_fav {
                        self.favorites.retain(|f| f.id != d.id);
                    } else {
                        self.favorites.push(d.clone());
                    }
                }
                if ui.button(ts("ℹ Scheda")).on_hover_text(ts("Dati del faro: dimensioni, peso, potenza, canali, ruote, link")).clicked() {
                    self.info_def = Some(d.clone());
                }
                if ui.button(ts("✏ Duplica e modifica")).clicked() {
                    let mut nd = d.clone();
                    nd.custom = true;
                    nd.id = String::new();
                    nd.name = crate::i18n::t(&format!("{} (mio)", d.name));
                    self.edit_def = Some(nd);
                }
                if d.custom && self.project.custom_defs.iter().any(|x| x.id == d.id) {
                    let used = self.project.fixtures.iter().any(|f| f.def_id == d.id);
                    if ui.add_enabled(!used, egui::Button::new("🗑")).on_hover_text(ts("Togli il modello dal progetto (solo se non usato)")).clicked() {
                        self.project.custom_defs.retain(|x| x.id != d.id);
                    }
                }
            });
        }
        ui.separator();
        if ui.button(ts("✏ Nuovo faro personalizzato…")).clicked() {
            self.edit_def = Some(FixtureDef {
                id: String::new(),
                name: ts("Il mio faro").into(),
                kind: FixtureKind::Par,
                channels: vec![
                    ch(ts("Dimmer"), ChannelKind::Dimmer, 0),
                    ch(ts("Rosso"), ChannelKind::Red, 0),
                    ch(ts("Verde"), ChannelKind::Green, 0),
                    ch(ts("Blu"), ChannelKind::Blue, 0),
                ],
                pan_range: 540.0,
                tilt_range: 270.0,
                color_wheel: vec![],
                beam_angle: 25.0,
                shutter_open: 255,
                shutter_closed: 0,
                strobe_min: 10,
                strobe_max: 250,
                custom: true,
                wheels: vec![],
        info: None,
            });
        }
        let open_lib = self.ofl_info.is_none() && self.ofl_rx.is_none();
        egui::CollapsingHeader::new(ts("Libreria fixture")).id_salt("ofl_lib").default_open(open_lib).show(ui, |ui| self.ofl_panel(ui));
    }

    /// Version, download and update of the Open Fixture Library.
    fn ofl_panel(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            ui.label(RichText::new(ts("Modelli dalla ")).size(11.0).color(Color32::GRAY));
            ui.hyperlink_to(RichText::new("Open Fixture Library").size(11.0), crate::ofl::SITE);
            ui.label(RichText::new(ts(", scaricati da Internet e aggiornati dall'app.")).size(11.0).color(Color32::GRAY));
        });
        match &self.ofl_info {
            Some(i) => {
                ui.label(RichText::new(crate::i18n::t(&format!("Versione del {} ({}) · {} modelli", i.day(), i.short(), self.ofl.len()))).size(11.5));
            }
            None => {
                ui.label(RichText::new(ts("Non ancora scaricata (circa 4 MB).")).size(11.5));
            }
        }
        if let Some(b) = self.ofl_busy {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(RichText::new(ts(b)).size(11.0));
            });
            if b.starts_with("Scarico") {
                let p = self.ofl_progress.lock().map(|p| *p).unwrap_or(0.0);
                ui.add(egui::ProgressBar::new(p).desired_width(220.0).show_percentage());
            }
        } else if let Some(e) = &self.ofl_error {
            ui.label(RichText::new(crate::i18n::t(&format!("Aggiornamento non riuscito: {e}"))).size(11.0).color(Color32::from_rgb(255, 140, 80)));
        }
        ui.horizontal(|ui| {
            let busy = self.ofl_rx.is_some();
            let label = if self.ofl_info.is_none() { ts("⬇ Scarica libreria") } else { ts("⟳ Controlla aggiornamenti") };
            if ui.add_enabled(!busy, egui::Button::new(label)).clicked() {
                self.start_ofl(crate::ofl::Update::Now, true);
            }
        });
        ui.checkbox(&mut self.prefs.ofl_auto, ts("Aggiorna automaticamente (controllo una volta al giorno)"));
    }

    pub fn ui_rig(&mut self, ctx: &egui::Context) {
        egui::SidePanel::left("catalog").default_width(340.0).min_width(260.0).max_width(520.0).resizable(true).show(ctx, |ui| {
            egui::ScrollArea::vertical().id_salt("cat_outer").show(ui, |ui| self.catalog_panel(ui));
        });

        self.fixture_panel(ctx);
        self.def_editor(ctx);

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.stage_mode, StageMode::Plan, ts("🗺 Pianta (trascina i fari)"));
                ui.selectable_value(&mut self.stage_mode, StageMode::Perspective, ts("🎭 Vista dal pubblico"));
                ui.separator();
                let st = &mut self.project.stage;
                ui.label(ts("Palco"));
                ui.add(egui::DragValue::new(&mut st.width).range(3.0..=40.0).speed(0.1).suffix(ts(" m largo")));
                ui.add(egui::DragValue::new(&mut st.depth).range(2.0..=25.0).speed(0.1).suffix(ts(" m profondo")));
                ui.label(ts("Americane"));
                ui.add(egui::DragValue::new(&mut st.truss_front).range(1.0..=12.0).speed(0.05).suffix(ts(" m fronte")));
                ui.add(egui::DragValue::new(&mut st.truss_back).range(1.0..=12.0).speed(0.05).suffix(ts(" m fondo")));
            });
            let avail = ui.available_size();
            let t = self.now();
            match self.stage_mode {
                StageMode::Plan => {
                    let before: Vec<(u32, f32)> = self.project.fixtures.iter().map(|f| (f.id, f.y)).collect();
                    let mut sel = self.sel_fixture;
                    let mut set = std::mem::take(&mut self.sel_set);
                    let mut drag = self.plan_drag.take();
                    let moved = stage::draw_plan(ui, vec2(avail.x, avail.y - 24.0), &mut self.project, &self.catalog, &self.states, &mut sel, &mut set, &mut drag);
                    self.sel_fixture = sel;
                    self.sel_set = set;
                    self.plan_drag = drag;
                    if moved {
                        self.sync_groups(&before);
                    }
                }
                StageMode::Perspective => {
                    if let Some(id) = stage::draw_perspective(ui, vec2(avail.x, avail.y - 24.0), &self.project, &self.catalog, &self.states, self.sel_fixture, t) {
                        let add = ui.input(|i| i.modifiers.ctrl || i.modifiers.command || i.modifiers.shift);
                        self.select_fixture(id, add);
                    }
                }
            }
            ui.label(
                RichText::new(ts("Pianta vista dall'alto (in basso il pubblico). Ctrl/Maiusc+clic: aggiungi alla selezione · trascina sul vuoto: selezione a riquadro · trascina un faro: sposta tutti i selezionati."))
                    .size(11.0)
                    .color(Color32::GRAY),
            );
        });
    }

    /// Keep automatic groups in sync with the position (front/back) after moving.
    fn sync_groups(&mut self, before: &[(u32, f32)]) {
        let depth = self.project.stage.depth;
        let cat = self.catalog.clone();
        let kinds: Vec<Option<FixtureKind>> = self.project.fixtures.iter().map(|f| self.project.def(&cat, &f.def_id).map(|d| d.kind)).collect();
        for (f, k) in self.project.fixtures.iter_mut().zip(kinds) {
            if let (Some(k), Some((_, oy))) = (k, before.iter().find(|b| b.0 == f.id)) {
                if f.group == auto_group(k, *oy, depth) {
                    f.group = auto_group(k, f.y, depth);
                }
            }
        }
    }

    fn fixture_panel(&mut self, ctx: &egui::Context) {
        egui::SidePanel::right("fixtures").default_width(340.0).resizable(true).show(ctx, |ui| {
            ui.heading(crate::i18n::t(&format!("Fari sul palco ({})", self.project.fixtures.len())));
            ui.horizontal(|ui| {
                if ui.button(ts("Ripatch automatico")).on_hover_text(ts("Riassegna gli indirizzi DMX in sequenza, senza sovrapposizioni")).clicked() {
                    let mut a = 1u16;
                    let cat = self.catalog.clone();
                    let sizes: Vec<u16> = self.project.fixtures.iter().map(|f| self.project.def(&cat, &f.def_id).map(|d| d.footprint()).unwrap_or(1)).collect();
                    for (f, n) in self.project.fixtures.iter_mut().zip(sizes) {
                        f.address = a.min(512);
                        a = a.saturating_add(n);
                    }
                    self.status = ts("Indirizzi DMX riassegnati").into();
                }
                if ui.button(ts("Tutti")).on_hover_text(ts("Seleziona tutti i fari")).clicked() {
                    self.sel_set = self.project.fixtures.iter().map(|f| f.id).collect();
                    self.sel_fixture = self.sel_set.iter().next().copied();
                }
                let used = self.project.next_address(&self.catalog) - 1;
                ui.label(RichText::new(crate::i18n::t(&format!("canali: {used}/512"))).color(Color32::GRAY));
            });
            for c in self.conflicts().iter().take(4) {
                ui.label(RichText::new(format!("⚠ {c}")).color(Color32::from_rgb(255, 150, 80)));
            }
            ui.separator();
            let order: Vec<u32> = Group::ALL.iter().flat_map(|g| self.project.fixtures.iter().filter(|f| f.group == *g).map(|f| f.id).collect::<Vec<_>>()).collect();
            let mut clicked: Option<(u32, bool, bool)> = None;
            egui::ScrollArea::vertical().max_height(ui.available_height() * 0.4).id_salt("fx_list").show(ui, |ui| {
                for g in Group::ALL {
                    let members: Vec<(u32, String, u16, u16)> = self
                        .project
                        .fixtures
                        .iter()
                        .filter(|f| f.group == g)
                        .map(|f| {
                            let n = self.project.def(&self.catalog, &f.def_id).map(|d| d.footprint()).unwrap_or(1);
                            (f.id, f.name.clone(), f.address, f.address + n - 1)
                        })
                        .collect();
                    if members.is_empty() {
                        continue;
                    }
                    ui.label(RichText::new(g.label()).strong().color(Color32::from_rgb(255, 200, 100)));
                    for (id, name, a, b) in members {
                        let txt = crate::i18n::t(&format!("{name}   DMX {a}-{b}"));
                        if ui.selectable_label(self.sel_set.contains(&id), txt).clicked() {
                            let (ctrl, shift) = ui.input(|i| (i.modifiers.ctrl || i.modifiers.command, i.modifiers.shift));
                            clicked = Some((id, ctrl, shift));
                        }
                    }
                }
            });
            if let Some((id, ctrl, shift)) = clicked {
                if shift {
                    // range from the main selection to this one, in list order
                    let a = self.sel_fixture.and_then(|s| order.iter().position(|x| *x == s)).unwrap_or(0);
                    let b = order.iter().position(|x| *x == id).unwrap_or(0);
                    let (lo, hi) = (a.min(b), a.max(b));
                    if !ctrl {
                        self.sel_set.clear();
                    }
                    for x in &order[lo..=hi] {
                        self.sel_set.insert(*x);
                    }
                    self.sel_fixture = Some(id);
                } else {
                    self.select_fixture(id, ctrl);
                }
            }
            ui.label(RichText::new(ts("Ctrl+clic: aggiungi/togli · Maiusc+clic: intervallo")).size(10.5).color(Color32::GRAY));
            ui.separator();
            // drop ids that no longer exist
            let ids: std::collections::BTreeSet<u32> = self.project.fixtures.iter().map(|f| f.id).collect();
            self.sel_set.retain(|x| ids.contains(x));
            if let Some(p) = self.sel_fixture {
                if !self.sel_set.contains(&p) {
                    self.sel_fixture = self.sel_set.iter().next().copied();
                }
            }
            egui::ScrollArea::vertical().id_salt("fx_props_scroll").show(ui, |ui| {
                if self.sel_set.len() > 1 {
                    self.multi_props(ui);
                } else {
                    self.single_props(ui);
                }
            });
        });
    }

    fn single_props(&mut self, ui: &mut egui::Ui) {
        let Some(id) = self.sel_fixture else {
            ui.label(ts("Seleziona un faro nella lista o sul palco."));
            return;
        };
        let defs = self.all_defs();
        let Some(idx) = self.project.fixtures.iter().position(|f| f.id == id) else { return };
        let mut delete = false;
        let mut duplicate = false;
        let mut identify = false;
        let mut show_info = None;
        {
            let f = &mut self.project.fixtures[idx];
            let def = defs.iter().find(|d| d.id == f.def_id);
            egui::Grid::new("fx_props").num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
                ui.label(ts("Nome"));
                ui.text_edit_singleline(&mut f.name);
                ui.end_row();
                ui.label(ts("Modello"));
                egui::ComboBox::from_id_salt("fx_model").width(200.0).selected_text(def.map(|d| d.name.clone()).unwrap_or_else(|| "?".into())).show_ui(ui, |ui| {
                    for d in &defs {
                        ui.selectable_value(&mut f.def_id, d.id.clone(), &d.name);
                    }
                });
                ui.end_row();
                ui.label(ts("Gruppo"));
                egui::ComboBox::from_id_salt("fx_group").width(200.0).selected_text(f.group.label()).show_ui(ui, |ui| {
                    for g in Group::ALL {
                        ui.selectable_value(&mut f.group, g, g.label());
                    }
                });
                ui.end_row();
                ui.label(ts("Indirizzo DMX"));
                ui.horizontal(|ui| {
                    ui.add(egui::DragValue::new(&mut f.address).range(1..=512));
                    if let Some(d) = def {
                        ui.label(crate::i18n::t(&format!("→ {} ({} canali)", f.address + d.footprint() - 1, d.footprint())));
                    }
                });
                ui.end_row();
                ui.label(ts("Posizione"));
                ui.horizontal(|ui| {
                    ui.add(egui::DragValue::new(&mut f.x).speed(0.05).range(-20.0..=20.0).prefix(ts("x ")).suffix(ts(" m")));
                    ui.add(egui::DragValue::new(&mut f.y).speed(0.05).range(-2.0..=25.0).prefix(ts("prof. ")).suffix(ts(" m")));
                });
                ui.end_row();
                ui.label(ts("Altezza"));
                ui.add(egui::DragValue::new(&mut f.z).speed(0.05).range(0.0..=12.0).suffix(ts(" m")));
                ui.end_row();
                ui.label(ts("Montaggio"));
                ui.horizontal(|ui| {
                    ui.radio_value(&mut f.hanging, false, ts("a terra"));
                    ui.radio_value(&mut f.hanging, true, ts("appeso"));
                });
                ui.end_row();
                let moving = def.map(|d| d.kind.is_moving()).unwrap_or(false);
                ui.label(if moving { ts("Pan di riposo") } else { ts("Rotazione") });
                ui.add(egui::Slider::new(&mut f.aim_pan, -180.0..=180.0).suffix("°"));
                ui.end_row();
                if !moving {
                    ui.label(ts("Inclinazione"));
                    ui.add(egui::Slider::new(&mut f.aim_tilt, -90.0..=90.0).suffix("°"))
                        .on_hover_text(ts("0° = dritto in verticale; positivo = verso il pubblico; negativo = verso il fondo"));
                    ui.end_row();
                }
            });
            if let Some(d) = def {
                ui.label(RichText::new(format!("{} — {}", d.kind.label(), d.capabilities())).size(11.0).color(Color32::GRAY));
            }
            ui.horizontal(|ui| {
                if ui.button(ts("💡 Identifica")).on_hover_text(ts("Accende solo questo faro per 3 secondi (anche sull'uscita DMX)")).clicked() {
                    identify = true;
                }
                if ui.add_enabled(def.is_some(), egui::Button::new(ts("ℹ Scheda"))).on_hover_text(ts("Dati del faro: dimensioni, peso, potenza, canali, ruote, link")).clicked() {
                    show_info = def.cloned();
                }
                if ui.button(ts("⧉ Duplica")).clicked() {
                    duplicate = true;
                }
                if ui.button(ts("🗑 Elimina")).clicked() {
                    delete = true;
                }
            });
        }
        if show_info.is_some() {
            self.info_def = show_info;
        }
        if identify {
            if let Ok(mut s) = self.shared.lock() {
                s.identify = Some((vec![id], Instant::now()));
            }
        }
        if duplicate {
            self.duplicate_selected();
        }
        if delete {
            self.project.fixtures.remove(idx);
            self.sel_fixture = None;
            self.sel_set.clear();
        }
    }

    fn duplicate_selected(&mut self) {
        let sel: Vec<FixtureInstance> = self.project.fixtures.iter().filter(|f| self.sel_set.contains(&f.id)).cloned().collect();
        let mut new_ids = std::collections::BTreeSet::new();
        for mut nf in sel {
            nf.id = self.project.next_id;
            self.project.next_id += 1;
            nf.x += 1.0;
            nf.address = self.project.next_address(&self.catalog);
            nf.name = crate::i18n::t(&format!("{} bis", nf.name));
            new_ids.insert(nf.id);
            self.project.fixtures.push(nf);
        }
        self.sel_fixture = new_ids.iter().next().copied();
        self.sel_set = new_ids;
    }

    /// Properties shared by several selected fixtures: a change applies to all.
    fn multi_props(&mut self, ui: &mut egui::Ui) {
        let ids = self.sel_set.clone();
        let n = ids.len();
        ui.label(RichText::new(crate::i18n::t(&format!("{n} fari selezionati"))).strong().size(15.0));
        ui.label(RichText::new(ts("Ogni modifica qui sotto vale per tutti i fari selezionati.")).size(11.0).color(Color32::GRAY));
        let first = self.project.fixtures.iter().find(|f| Some(f.id) == self.sel_fixture || ids.contains(&f.id)).cloned();
        let Some(first) = first else { return };
        let sel: Vec<&FixtureInstance> = self.project.fixtures.iter().filter(|f| ids.contains(&f.id)).collect();
        let same_group = sel.iter().all(|f| f.group == first.group);
        let mut group = first.group;
        let mut z = first.z;
        let mut hanging = first.hanging;
        let mut aim_pan = first.aim_pan;
        let mut aim_tilt = first.aim_tilt;
        let mut y = first.y;
        let (mut ch_group, mut ch_z, mut ch_hang, mut ch_pan, mut ch_tilt, mut ch_y) = (false, false, false, false, false, false);
        egui::Grid::new("multi_props").num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
            ui.label(ts("Gruppo"));
            egui::ComboBox::from_id_salt("multi_group")
                .width(200.0)
                .selected_text(if same_group { group.label() } else { ts("(misto)") })
                .show_ui(ui, |ui| {
                    for g in Group::ALL {
                        if ui.selectable_value(&mut group, g, g.label()).clicked() {
                            ch_group = true;
                        }
                    }
                });
            ui.end_row();
            ui.label(ts("Profondità"));
            ch_y = ui.add(egui::DragValue::new(&mut y).speed(0.05).range(-2.0..=25.0).suffix(ts(" m"))).changed();
            ui.end_row();
            ui.label(ts("Altezza"));
            ch_z = ui.add(egui::DragValue::new(&mut z).speed(0.05).range(0.0..=12.0).suffix(ts(" m"))).changed();
            ui.end_row();
            ui.label(ts("Montaggio"));
            ui.horizontal(|ui| {
                ch_hang |= ui.radio_value(&mut hanging, false, ts("a terra")).changed();
                ch_hang |= ui.radio_value(&mut hanging, true, ts("appeso")).changed();
            });
            ui.end_row();
            ui.label(ts("Pan / rotazione"));
            ch_pan = ui.add(egui::Slider::new(&mut aim_pan, -180.0..=180.0).suffix("°")).changed();
            ui.end_row();
            ui.label(ts("Inclinazione"));
            ch_tilt = ui.add(egui::Slider::new(&mut aim_tilt, -90.0..=90.0).suffix("°")).changed();
            ui.end_row();
        });
        let mut identify = false;
        let mut dup = false;
        let mut del = false;
        let mut align_row = false;
        let mut spread = false;
        let mut mirror = false;
        let mut repatch = false;
        ui.horizontal_wrapped(|ui| {
            align_row = ui.button(ts("⇔ Allinea in riga")).on_hover_text(ts("Stessa profondità e altezza del faro principale")).clicked();
            spread = ui.button(ts("↔ Distribuisci")).on_hover_text(ts("Spaziatura uguale tra il più a sinistra e il più a destra")).clicked();
            mirror = ui.button(ts("⇋ Specchia")).on_hover_text(ts("Ribalta destra/sinistra rispetto al centro del palco")).clicked();
        });
        ui.horizontal_wrapped(|ui| {
            repatch = ui.button(ts("Indirizzi in sequenza")).on_hover_text(ts("Indirizzi DMX consecutivi a partire dal primo selezionato")).clicked();
            identify = ui.button(ts("💡 Identifica")).clicked();
            dup = ui.button(ts("⧉ Duplica")).clicked();
            del = ui.button(ts("🗑 Elimina")).clicked();
        });
        let cat = self.catalog.clone();
        let sizes: std::collections::HashMap<u32, u16> = self
            .project
            .fixtures
            .iter()
            .map(|f| (f.id, self.project.def(&cat, &f.def_id).map(|d| d.footprint()).unwrap_or(1)))
            .collect();
        let before: Vec<(u32, f32)> = self.project.fixtures.iter().map(|f| (f.id, f.y)).collect();
        let (mut minx, mut maxx) = (f32::MAX, f32::MIN);
        for f in self.project.fixtures.iter().filter(|f| ids.contains(&f.id)) {
            minx = minx.min(f.x);
            maxx = maxx.max(f.x);
        }
        let mut order: Vec<u32> = ids.iter().copied().collect();
        order.sort_by(|a, b| {
            let fx = |id: &u32| self.project.fixtures.iter().find(|f| f.id == *id).map(|f| f.x).unwrap_or(0.0);
            fx(a).total_cmp(&fx(b))
        });
        let mut addr = self.project.fixtures.iter().filter(|f| ids.contains(&f.id)).map(|f| f.address).min().unwrap_or(1);
        if repatch {
            let mut by_addr: Vec<(u16, u32)> = self.project.fixtures.iter().filter(|f| ids.contains(&f.id)).map(|f| (f.address, f.id)).collect();
            by_addr.sort();
            for (_, id) in by_addr {
                if let Some(f) = self.project.fixtures.iter_mut().find(|f| f.id == id) {
                    f.address = addr.min(512);
                    addr = addr.saturating_add(*sizes.get(&id).unwrap_or(&1));
                }
            }
        }
        for f in self.project.fixtures.iter_mut().filter(|f| ids.contains(&f.id)) {
            if ch_group {
                f.group = group;
            }
            if ch_z {
                f.z = z;
            }
            if ch_y {
                f.y = y;
            }
            if ch_hang {
                f.hanging = hanging;
            }
            if ch_pan {
                f.aim_pan = aim_pan;
            }
            if ch_tilt {
                f.aim_tilt = aim_tilt;
            }
            if align_row {
                f.y = first.y;
                f.z = first.z;
            }
            if mirror {
                f.x = -f.x;
                f.aim_pan = -f.aim_pan;
            }
            if spread && order.len() > 1 {
                let k = order.iter().position(|x| *x == f.id).unwrap_or(0) as f32;
                f.x = minx + (maxx - minx) * k / (order.len() - 1) as f32;
            }
        }
        if ch_y || align_row {
            self.sync_groups(&before);
        }
        if identify {
            if let Ok(mut s) = self.shared.lock() {
                s.identify = Some((ids.iter().copied().collect(), Instant::now()));
            }
        }
        if dup {
            self.duplicate_selected();
        }
        if del {
            self.project.fixtures.retain(|f| !ids.contains(&f.id));
            self.sel_set.clear();
            self.sel_fixture = None;
        }
    }

    fn def_editor(&mut self, ctx: &egui::Context) {
        let Some(mut d) = self.edit_def.take() else { return };
        let mut open = true;
        let mut save = false;
        let mut cancel = false;
        egui::Window::new(ts("Faro personalizzato")).open(&mut open).default_width(520.0).resizable(true).show(ctx, |ui| {
            egui::Grid::new("def_head").num_columns(2).show(ui, |ui| {
                ui.label(ts("Nome modello"));
                ui.text_edit_singleline(&mut d.name);
                ui.end_row();
                ui.label(ts("Tipo"));
                egui::ComboBox::from_id_salt("def_kind").selected_text(d.kind.label()).show_ui(ui, |ui| {
                    for k in FixtureKind::ALL {
                        ui.selectable_value(&mut d.kind, k, k.label());
                    }
                });
                ui.end_row();
                ui.label(ts("Apertura fascio"));
                ui.add(egui::Slider::new(&mut d.beam_angle, 2.0..=70.0).suffix("°"));
                ui.end_row();
                if d.kind.is_moving() {
                    ui.label(ts("Escursione pan / tilt"));
                    ui.horizontal(|ui| {
                        ui.add(egui::DragValue::new(&mut d.pan_range).range(90.0..=720.0).suffix("°"));
                        ui.add(egui::DragValue::new(&mut d.tilt_range).range(90.0..=360.0).suffix("°"));
                    });
                    ui.end_row();
                }
                if d.has(ChannelKind::Shutter) || d.has(ChannelKind::Strobe) {
                    ui.label(ts("Shutter aperto / chiuso"));
                    ui.horizontal(|ui| {
                        ui.add(egui::DragValue::new(&mut d.shutter_open));
                        ui.add(egui::DragValue::new(&mut d.shutter_closed));
                    });
                    ui.end_row();
                    ui.label(ts("Strobo lento → veloce"));
                    ui.horizontal(|ui| {
                        ui.add(egui::DragValue::new(&mut d.strobe_min));
                        ui.add(egui::DragValue::new(&mut d.strobe_max));
                    });
                    ui.end_row();
                }
            });
            ui.separator();
            ui.label(RichText::new(ts("Canali DMX (nell'ordine del manuale del faro)")).strong());
            let mut remove = None;
            let mut swap = None;
            egui::ScrollArea::vertical().max_height(260.0).show(ui, |ui| {
                egui::Grid::new("def_ch").num_columns(5).striped(true).show(ui, |ui| {
                    let n = d.channels.len();
                    for (i, c) in d.channels.iter_mut().enumerate() {
                        ui.label(format!("{}", i + 1));
                        ui.add(egui::TextEdit::singleline(&mut c.name).desired_width(130.0));
                        egui::ComboBox::from_id_salt(("chk", i)).width(150.0).selected_text(c.kind.label()).show_ui(ui, |ui| {
                            for k in ChannelKind::ALL {
                                ui.selectable_value(&mut c.kind, k, k.label());
                            }
                        });
                        ui.add(egui::DragValue::new(&mut c.default)).on_hover_text(ts("Valore di riposo / fisso"));
                        ui.horizontal(|ui| {
                            if i > 0 && ui.small_button("↑").clicked() {
                                swap = Some((i, i - 1));
                            }
                            if i + 1 < n && ui.small_button("↓").clicked() {
                                swap = Some((i, i + 1));
                            }
                            if ui.small_button("×").clicked() {
                                remove = Some(i);
                            }
                        });
                        ui.end_row();
                    }
                });
            });
            if let Some((a, b)) = swap {
                d.channels.swap(a, b);
            }
            if let Some(i) = remove {
                d.channels.remove(i);
            }
            if ui.button(ts("+ Aggiungi canale")).clicked() {
                d.channels.push(ch(&crate::i18n::t(&format!("Canale {}", d.channels.len() + 1)), ChannelKind::Other, 0));
            }
            if d.has(ChannelKind::ColorWheel) {
                ui.separator();
                ui.label(RichText::new(ts("Ruota colori: valore DMX → colore")).strong());
                let mut rm = None;
                ui.horizontal_wrapped(|ui| {
                    for (i, s) in d.color_wheel.iter_mut().enumerate() {
                        ui.add(egui::DragValue::new(&mut s.value));
                        ui.color_edit_button_srgb(&mut s.color);
                        if ui.small_button("×").clicked() {
                            rm = Some(i);
                        }
                        ui.add_space(6.0);
                    }
                });
                if let Some(i) = rm {
                    d.color_wheel.remove(i);
                }
                if ui.button(ts("+ Colore")).clicked() {
                    let v = d.color_wheel.last().map(|s| s.value.saturating_add(10)).unwrap_or(0);
                    d.color_wheel.push(ColorSlot { value: v, color: [255, 255, 255] });
                }
            }
            ui.separator();
            ui.horizontal(|ui| {
                if ui.button(RichText::new(ts("💾 Salva modello")).strong()).clicked() {
                    save = true;
                }
                if ui.button(ts("Annulla")).clicked() {
                    cancel = true;
                }
            });
        });
        if save {
            d.custom = true;
            if d.id.is_empty() {
                d.id = format!("custom-{}-{}", self.project.custom_defs.len() + 1, d.name.len());
                while self.project.custom_defs.iter().any(|x| x.id == d.id) {
                    d.id.push('x');
                }
            }
            if let Some(x) = self.project.custom_defs.iter_mut().find(|x| x.id == d.id) {
                *x = d.clone();
            } else {
                self.project.custom_defs.push(d.clone());
            }
            if let Some(f) = self.favorites.iter_mut().find(|x| x.id == d.id) {
                *f = d.clone();
            }
            self.catalog_pick = d.id.clone();
            self.ofl_pick = None;
            self.status = crate::i18n::t(&format!("Modello «{}» salvato nel progetto", d.name));
            return;
        }
        if open && !cancel {
            self.edit_def = Some(d);
        }
    }
}
