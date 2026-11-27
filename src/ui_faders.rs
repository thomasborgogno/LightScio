//! "Canali" panel under the stage: one fader per DMX channel of the selected light
//! (all the selected lights of the same model move together), each with a flash
//! button. Touched channels override the show until they are released.

use crate::app::*;
#[allow(unused_imports)]
use crate::i18n::{t, ts};
use crate::model::*;
use egui::{pos2, vec2, Color32, Rect, RichText, Sense, Stroke, StrokeKind};

const STRIP_W: f32 = 50.0;
const MANUAL: Color32 = Color32::from_rgb(255, 170, 40);
const SHOW: Color32 = Color32::from_rgb(90, 125, 170);
const FLASH_H: f32 = 30.0;

/// What the user did on one fader.
#[derive(Default)]
struct FaderOut {
    /// New value from a drag.
    value: Option<u8>,
    double: bool,
    release: bool,
}

/// Vertical fader 0-255. Grabbing the knob moves it from where it is; pressing
/// elsewhere on the track jumps there first.
fn fader(ui: &mut egui::Ui, value: u8, manual: bool, height: f32) -> (egui::Response, FaderOut) {
    let (rect, resp) = ui.allocate_exact_size(vec2(STRIP_W - 8.0, height), Sense::click_and_drag());
    let (top, bot) = (rect.top() + 7.0, rect.bottom() - 7.0);
    let span = (bot - top).max(1.0);
    let y_of = |v: f32| bot - span * v / 255.0;
    let knob_at = |v: f32| Rect::from_center_size(pos2(rect.center().x, y_of(v)), vec2(rect.width() - 6.0, 14.0));
    let mut out = FaderOut { double: resp.double_clicked(), release: resp.secondary_clicked() || resp.long_touched(), ..Default::default() };
    let key = resp.id.with("grab");
    if resp.drag_started() {
        if let Some(o) = ui.input(|i| i.pointer.press_origin()) {
            let start = if knob_at(value as f32).contains(o) { value as f32 } else { ((bot - o.y) / span * 255.0).clamp(0.0, 255.0) };
            ui.data_mut(|d| d.insert_temp(key, (start, o.y)));
        }
    }
    if resp.dragged() {
        if let (Some((start, oy)), Some(p)) = (ui.data(|d| d.get_temp::<(f32, f32)>(key)), resp.interact_pointer_pos()) {
            out.value = Some((start + (oy - p.y) / span * 255.0).clamp(0.0, 255.0).round() as u8);
        }
    }
    let shown = out.value.unwrap_or(value) as f32;
    let col = if manual || out.value.is_some() { MANUAL } else { SHOW };
    let p = ui.painter();
    let track = Rect::from_x_y_ranges(rect.center().x - 3.0..=rect.center().x + 3.0, top..=bot);
    p.rect_filled(track, 3.0, Color32::from_rgb(10, 10, 14));
    p.rect_filled(Rect::from_x_y_ranges(track.x_range(), y_of(shown)..=bot), 3.0, col.gamma_multiply(0.75));
    let knob = knob_at(shown);
    let hot = resp.hovered() || resp.dragged();
    p.rect_filled(knob, 3.0, if hot { Color32::from_rgb(228, 230, 240) } else { Color32::from_rgb(180, 183, 196) });
    p.hline(knob.x_range().shrink(4.0), knob.center().y, Stroke::new(2.0_f32, col));
    (resp, out)
}

/// Flash button: lit while held. Returns (held, double clicked).
fn flash_button(ui: &mut egui::Ui, at_full: bool) -> (egui::Response, bool, bool) {
    let (rect, resp) = ui.allocate_exact_size(vec2(STRIP_W - 8.0, FLASH_H), Sense::click());
    let held = resp.is_pointer_button_down_on();
    let fill = if held {
        MANUAL
    } else if resp.hovered() {
        Color32::from_rgb(62, 64, 78)
    } else {
        Color32::from_rgb(40, 42, 52)
    };
    let p = ui.painter();
    p.rect_filled(rect, 4.0, fill);
    if at_full && !held {
        p.rect_stroke(rect, 4.0, Stroke::new(1.5_f32, MANUAL), StrokeKind::Inside);
    }
    let txt = if held { Color32::BLACK } else { Color32::from_rgb(230, 230, 240) };
    p.text(rect.center(), egui::Align2::CENTER_CENTER, "⚡", egui::FontId::proportional(15.0), txt);
    let double = resp.double_clicked();
    (resp, held, double)
}

/// What the current value of a channel does (from the fixture library).
fn cap_text(c: &ChannelDef, v: u8) -> Option<String> {
    let cap = c.caps.iter().find(|k| k.start <= v && v <= k.end)?;
    let parts: Vec<&str> = [cap.typ.as_str(), cap.what.as_str(), cap.values.as_str(), cap.comment.as_str()].into_iter().filter(|s| !s.is_empty()).collect();
    Some(format!("{}-{}: {}", cap.start, cap.end, parts.join(" · ")))
}

impl App {
    /// Fixtures in the order of the list on the right (by group).
    pub fn list_order(&self) -> Vec<u32> {
        Group::ALL.iter().flat_map(|g| self.project.fixtures.iter().filter(|f| f.group == *g).map(|f| f.id).collect::<Vec<_>>()).collect()
    }

    pub fn faders_panel(&mut self, ctx: &egui::Context) {
        if !self.show_faders {
            return;
        }
        egui::TopBottomPanel::bottom("fader_panel").resizable(true).default_height(300.0).height_range(190.0..=620.0).show(ctx, |ui| self.faders_ui(ui));
    }

    fn faders_ui(&mut self, ui: &mut egui::Ui) {
        // fill the panel: it would otherwise shrink to the faders, which follow the panel
        ui.set_min_height(ui.available_height());
        let order = self.list_order();
        let main = self.sel_fixture.and_then(|id| self.project.fixtures.iter().find(|f| f.id == id)).cloned();
        let def = main.as_ref().and_then(|f| self.project.def(&self.catalog, &f.def_id)).cloned();
        // the faders move every selected light of the same model as the main one
        let targets: Vec<u32> = match &main {
            Some(m) => order.iter().copied().filter(|id| self.sel_set.contains(id) && self.project.fixtures.iter().any(|f| f.id == *id && f.def_id == m.def_id)).collect(),
            None => vec![],
        };
        // models on stage, for the "same lights" menu: (model id, name, count)
        let mut models: Vec<(String, String, usize)> = vec![];
        for f in &self.project.fixtures {
            match models.iter_mut().find(|m| m.0 == f.def_id) {
                Some(m) => m.2 += 1,
                None => {
                    let name = self.project.def(&self.catalog, &f.def_id).map(|d| d.name.clone()).unwrap_or_else(|| "?".into());
                    models.push((f.def_id.clone(), name, 1));
                }
            }
        }

        let mut go: Option<u32> = None;
        let mut pick_model: Option<String> = None;
        let mut release_shown = false;
        let mut release_all = false;
        ui.add_space(2.0);
        ui.horizontal(|ui| {
            let pos = main.as_ref().and_then(|m| order.iter().position(|x| *x == m.id));
            let n = order.len();
            if ui.add_enabled(n > 0, egui::Button::new("◀")).on_hover_text(ts("Faro precedente")).clicked() {
                go = Some(order[pos.map(|i| (i + n - 1) % n).unwrap_or(n - 1)]);
            }
            if ui.add_enabled(n > 0, egui::Button::new("▶")).on_hover_text(ts("Faro successivo")).clicked() {
                go = Some(order[pos.map(|i| (i + 1) % n).unwrap_or(0)]);
            }
            match (&main, &def) {
                (Some(m), Some(d)) => {
                    ui.label(RichText::new(&m.name).strong());
                    ui.label(RichText::new(format!("{} · DMX {}", d.name, m.address)).color(Color32::GRAY));
                }
                _ => {
                    ui.label(RichText::new(ts("Nessun faro selezionato")).color(Color32::GRAY));
                }
            }
            ui.separator();
            let whole = models.iter().find(|x| main.as_ref().map(|m| m.def_id == x.0).unwrap_or(false) && targets.len() == x.2 && x.2 > 1);
            let label = match whole {
                Some(x) => t(&format!("Tutti i {} ({})", x.1, x.2)),
                None => ts("Fari uguali…").to_string(),
            };
            egui::ComboBox::from_id_salt("fader_models").selected_text(label).width(200.0).show_ui(ui, |ui| {
                for (id, name, n) in &models {
                    if ui.selectable_label(whole.map(|x| &x.0 == id).unwrap_or(false), format!("{name} × {n}")).clicked() {
                        pick_model = Some(id.clone());
                    }
                }
            })
            .response
            .on_hover_text(ts("Seleziona tutti i fari dello stesso modello: i fader li muovono insieme"));
            if targets.len() > 1 {
                ui.label(RichText::new(t(&format!("{} fari insieme", targets.len()))).color(MANUAL));
            }
            let other = self.sel_set.len().saturating_sub(targets.len());
            if main.is_some() && other > 0 {
                ui.label(RichText::new(t(&format!("({} di altro modello esclusi)", other))).color(Color32::GRAY))
                    .on_hover_text(ts("I fader valgono solo per i fari selezionati dello stesso modello del faro principale"));
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                release_all = ui.add_enabled(!self.manual.values.is_empty(), egui::Button::new(ts("Rilascia tutto"))).on_hover_text(ts("Tutti i canali manuali tornano allo show")).clicked();
                let any = targets.iter().any(|id| self.manual.values.keys().any(|k| k.0 == *id));
                release_shown = ui.add_enabled(any, egui::Button::new(ts("↺ Rilascia"))).on_hover_text(ts("I canali dei fari mostrati tornano allo show")).clicked();
            });
        });

        // edits collected while drawing, applied to all the targets afterwards
        let mut set: Vec<(usize, Option<u8>)> = vec![];
        let mut flash: Vec<usize> = vec![];
        if let (Some(m), Some(d)) = (&main, &def) {
            let uni = self.shared.lock().map(|s| s.universe).unwrap_or([0; 512]);
            ui.label(
                RichText::new(ts("Trascina il fader · doppio tap sul fader: valore di riposo · tasto destro o tieni premuto: torna allo show · ⚡ tieni premuto: flash · doppio tap su ⚡: al massimo"))
                    .size(10.5)
                    .color(Color32::GRAY),
            );
            // measured here: inside the horizontal scroll area the height is not the panel's
            // (two label rows, value box, flash button, spacing, frame margins, scroll bar)
            let fixed = 2.0 * 15.0 + ui.spacing().interact_size.y + FLASH_H + 5.0 * 3.0 + 8.0 + 12.0;
            let h = (ui.available_height() - fixed).max(60.0);
            egui::ScrollArea::horizontal().id_salt("fader_strips").auto_shrink([false, false]).show(ui, |ui| {
                ui.horizontal_top(|ui| {
                    for (i, c) in d.channels.iter().enumerate() {
                        let addr = m.address as usize + i;
                        let manual = self.manual.values.get(&(m.id, i)).copied();
                        let out_v = if (1..=512).contains(&addr) { uni[addr - 1] } else { 0 };
                        let value = manual.unwrap_or(out_v);
                        let fill = if manual.is_some() { Color32::from_rgb(52, 40, 22) } else { Color32::from_rgb(28, 29, 37) };
                        egui::Frame::new().fill(fill).corner_radius(4.0).inner_margin(egui::Margin::same(4)).show(ui, |ui| ui.vertical(|ui| {
                            ui.set_width(STRIP_W - 8.0);
                            ui.spacing_mut().item_spacing.y = 3.0;
                            let mut info = t(&format!("{} — {}\nDMX {} · riposo {}", c.name, c.kind.label(), addr, c.default));
                            if let Some(s) = cap_text(c, value) {
                                info.push('\n');
                                info.push_str(&s);
                            }
                            ui.label(RichText::new(format!("{}", i + 1)).size(10.0).color(Color32::GRAY)).on_hover_text(&info);
                            ui.add(egui::Label::new(RichText::new(&c.name).size(10.5)).truncate()).on_hover_text(&info);
                            let (resp, f) = fader(ui, value, manual.is_some(), h);
                            resp.on_hover_text(&info);
                            if let Some(v) = f.value {
                                set.push((i, Some(v)));
                            }
                            if f.double {
                                set.push((i, Some(c.default)));
                            }
                            if f.release {
                                set.push((i, None));
                            }
                            let mut v = value;
                            if ui.add_sized(vec2(STRIP_W - 8.0, ui.spacing().interact_size.y), egui::DragValue::new(&mut v).range(0..=255)).changed() {
                                set.push((i, Some(v)));
                            }
                            let (resp, held, double) = flash_button(ui, manual == Some(255));
                            resp.on_hover_text(ts("Tieni premuto: flash al massimo · doppio tap: fissa al massimo"));
                            if held {
                                flash.push(i);
                            }
                            if double {
                                set.push((i, Some(255)));
                            }
                        }));
                    }
                });
            });
        } else {
            ui.add_space(8.0);
            ui.label(RichText::new(ts("Seleziona un faro nella lista o sul palco: qui compaiono i suoi canali DMX.")).color(Color32::GRAY));
        }

        for id in &targets {
            for (i, v) in &set {
                match v {
                    Some(v) => {
                        self.manual.values.insert((*id, *i), *v);
                    }
                    None => {
                        self.manual.values.remove(&(*id, *i));
                    }
                }
            }
            for i in &flash {
                self.manual.flash.insert((*id, *i));
            }
        }
        if release_shown {
            for id in &targets {
                self.manual.release(*id);
            }
        }
        if release_all {
            self.manual.values.clear();
        }
        if let Some(id) = go {
            self.sel_set.clear();
            self.sel_set.insert(id);
            self.sel_fixture = Some(id);
        }
        if let Some(model) = pick_model {
            let ids: Vec<u32> = order.iter().copied().filter(|id| self.project.fixtures.iter().any(|f| f.id == *id && f.def_id == model)).collect();
            self.sel_fixture = ids.first().copied();
            self.sel_set = ids.into_iter().collect();
        }
    }
}
