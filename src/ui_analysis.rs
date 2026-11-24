//! Analysis editor (Brani e analisi tab): waveforms of the tracks, bar grid
//! with the first downbeat and extra downbeats, song parts edited directly
//! on the tracks before the show is generated.

use crate::app::*;
#[allow(unused_imports)]
use crate::i18n::{t, ts};
use crate::lanes::{self, Lane};
use crate::model::*;
use egui::{pos2, vec2, Color32, Rect, RichText, Sense, Stroke, StrokeKind};
use std::sync::Arc;

enum EAct {
    Seek(f64),
    Select(Option<usize>),
    Start(EdDrag),
    DragTo(f64, f32, bool),
    End,
    View(f64, f32),
    AddDownbeat(f64),
    DelDownbeat(usize),
    Split(f64),
    Merge(usize),
    TrackZoom(f32),
}

impl App {
    fn sync_metronome(&mut self) {
        let Some(a) = self.cur_song().and_then(|s| s.analysis.as_ref()) else { return };
        let key = format!("{}|{}|{}|{:?}|{}", self.song, a.bpm, a.offset, a.downbeats, a.beats_per_bar);
        if key != self.metro_key {
            let beats = Arc::new(a.beat_times());
            self.player.set_metronome(Some(beats));
            self.metro_key = key;
        }
    }

    pub fn analysis_editor(&mut self, ui: &mut egui::Ui) {
        self.sync_metronome();
        let t_now = self.now();
        let playing = self.is_playing();
        // ---------------- toolbar: transport + grid
        let mut add_db = false;
        let mut clear_db = false;
        let mut align = false;
        ui.horizontal(|ui| {
            let big = |s: &str| RichText::new(s).size(17.0);
            if ui.button(big("⏮")).clicked() {
                self.seek(0.0);
                self.ed.start = 0.0;
            }
            if ui.button(big(if playing { "⏸" } else { "▶" })).on_hover_text(ts("Ascolta (barra spaziatrice)")).clicked() {
                if playing {
                    self.pause()
                } else {
                    self.play()
                }
            }
            if ui.button(big("⏹")).clicked() {
                self.stop();
            }
            if ui.add_enabled(!self.undo.is_empty(), egui::Button::new(ts("↶ Annulla"))).on_hover_text(ts("Annulla l'ultima modifica (Ctrl+Z)")).clicked() {
                self.do_undo();
                self.ed.sel = None;
            }
            let bb = self.cur_song().and_then(|s| s.analysis.as_ref()).map(|a| a.bar_beat(t_now)).unwrap_or_default();
            ui.label(RichText::new(fmt_time(t_now)).monospace().size(18.0).color(Color32::from_rgb(255, 210, 90)));
            ui.label(RichText::new(crate::i18n::t(&format!("battuta {bb}"))).monospace());
            let mut metro = self.player.metronome_on();
            if ui.toggle_value(&mut metro, ts("🥁 Click")).on_hover_text(ts("Aggiunge un click sui battiti all'ascolto: serve a verificare la griglia")).changed() {
                self.player.set_metronome_on(metro);
            }
            ui.separator();
            let si = self.song;
            if let Some(a) = self.project.songs.get_mut(si).and_then(|s| s.analysis.as_mut()) {
                ui.label(ts("Tempo"));
                ui.add(egui::DragValue::new(&mut a.bpm).range(30.0..=300.0).speed(0.02).suffix(ts(" BPM")));
                ui.label(ts("Primo battere"));
                ui.add(egui::DragValue::new(&mut a.offset).range(0.0..=60.0).speed(0.002).suffix(ts(" s")))
                    .on_hover_text(ts("Puoi anche trascinare la bandierina gialla sul righello"));
                ui.label(ts("Battiti/battuta"));
                ui.add(egui::DragValue::new(&mut a.beats_per_bar).range(1..=12));
            }
            ui.separator();
            add_db = ui.button(ts("＋ Nuovo battere qui")).on_hover_text(ts("Fa ripartire le battute dal punto di ascolto (per battute più corte). Anche doppio clic sul righello.")).clicked();
            let has_db = self.cur_song().and_then(|s| s.analysis.as_ref()).map(|a| !a.downbeats.is_empty()).unwrap_or(false);
            if has_db {
                clear_db = ui.button(ts("Togli i battere aggiunti")).clicked();
            }
            align = ui.button(ts("⇤ Allinea le parti alle battute")).on_hover_text(ts("Sposta ogni confine tra le parti sulla battuta più vicina")).clicked();
        });
        if add_db || clear_db || align {
            self.push_undo();
            if let Some(s) = self.cur_song_mut() {
                if let Some(a) = s.analysis.as_mut() {
                    if add_db {
                        let t = a.snap(t_now, 1.0);
                        if t > a.offset + 0.05 {
                            a.downbeats.push(t);
                            a.downbeats.sort_by(|x, y| x.total_cmp(y));
                        }
                    }
                    if clear_db {
                        a.downbeats.clear();
                    }
                }
                if align {
                    if let Some(a) = s.analysis.clone() {
                        let n = s.sections.len();
                        for i in 1..n {
                            let t = a.snap_bar(s.sections[i].start);
                            s.sections[i].start = t;
                            s.sections[i - 1].end = t;
                        }
                    }
                }
            }
        }

        // ---------------- selected part
        let mut sec_act: Option<crate::ui_songs::SecAct> = None;
        let mut del_sec = false;
        ui.horizontal(|ui| {
            let si = self.song;
            let sel = self.ed.sel;
            let song = &mut self.project.songs[si];
            let a = song.analysis.clone().unwrap_or_default();
            let bar = a.bar_len();
            let nsec = song.sections.len();
            match sel.and_then(|i| song.sections.get_mut(i).map(|s| (i, s))) {
                Some((i, s)) => {
                    let c = s.kind.color();
                    let (r, _) = ui.allocate_exact_size(vec2(12.0, 12.0), Sense::hover());
                    ui.painter().rect_filled(r, 2.0, Color32::from_rgb(c[0], c[1], c[2]));
                    ui.add(egui::TextEdit::singleline(&mut s.label).desired_width(150.0));
                    egui::ComboBox::from_id_salt("ed_kind").selected_text(s.kind.label()).show_ui(ui, |ui| {
                        for k in SectionKind::ALL {
                            ui.selectable_value(&mut s.kind, k, k.label());
                        }
                    });
                    ui.label(ts("Intensità"));
                    ui.add(egui::Slider::new(&mut s.intensity, 0.0..=1.0).show_value(false));
                    ui.label(intensity_label(s.intensity));
                    ui.label(RichText::new(crate::i18n::t(&format!("battute {}–{}", a.bar_number(s.start + 0.01), a.bar_number(s.end - 0.01)))).monospace());
                    if ui.button(ts("▶ Ascolta la parte")).clicked() {
                        sec_act = Some(crate::ui_songs::SecAct::Play(s.start, s.end));
                    }
                    if ui.button(ts("⇥ Ascolta l'attacco")).on_hover_text(ts("Da una battuta prima a una dopo l'inizio della parte")).clicked() {
                        sec_act = Some(crate::ui_songs::SecAct::Play((s.start - bar).max(0.0), s.start + bar));
                    }
                    if ui.add_enabled(i + 1 < nsec, egui::Button::new(ts("⊕ Unisci alla successiva"))).clicked() {
                        sec_act = Some(crate::ui_songs::SecAct::Merge(i));
                    }
                    if ui.button(ts("✂ Dividi al punto di ascolto")).clicked() {
                        sec_act = Some(crate::ui_songs::SecAct::SplitAt(t_now));
                    }
                    if ui.add_enabled(nsec > 1, egui::Button::new("🗑")).on_hover_text(ts("Elimina la parte (si unisce alla precedente)")).clicked() {
                        del_sec = true;
                    }
                }
                None => {
                    ui.label(
                        RichText::new(ts("Clicca una parte per modificarla · trascina i bordi delle parti (si agganciano alle battute) · doppio clic su una parte: dividila · trascina la bandierina gialla per spostare il primo battere · doppio clic sul righello: nuovo battere · tasto destro su un battere aggiunto: toglilo"))
                            .size(11.0)
                            .color(Color32::GRAY),
                    );
                }
            }
        });
        if let Some(a) = sec_act {
            self.section_action(a);
        }
        if del_sec {
            if let Some(i) = self.ed.sel {
                self.push_undo();
                if let Some(s) = self.cur_song_mut() {
                    if s.sections.len() > 1 && i < s.sections.len() {
                        let removed = s.sections.remove(i);
                        if i > 0 {
                            s.sections[i - 1].end = removed.end;
                        } else {
                            s.sections[0].start = removed.start;
                        }
                    }
                }
                self.ed.sel = None;
            }
        }
        ui.horizontal(|ui| {
            ui.label(ts("Zoom ↔"));
            ui.add(egui::Slider::new(&mut self.ed.pps, 3.0..=300.0).logarithmic(true).show_value(false));
            ui.label(ts("Zoom ↕ tracce"));
            ui.add(egui::Slider::new(&mut self.project.view.track_h, 8.0..=160.0).show_value(false));
            ui.checkbox(&mut self.project.view.show_energy, ts("Energia"));
            ui.checkbox(&mut self.ed.follow, ts("Segui"));
            if !self.waves.contains_key(&self.song) {
                ui.label(RichText::new(ts("(forme d'onda in caricamento…)")).color(Color32::GRAY));
            }
        });

        // ---------------- timeline
        let view_h = ui.available_height();
        egui::ScrollArea::vertical().id_salt("ed_scroll").auto_shrink([false, false]).show(ui, |ui| self.editor_lanes(ui, view_h));
    }

    fn editor_lanes(&mut self, ui: &mut egui::Ui, view_h: f32) {
        let si = self.song;
        let Some(song) = self.project.songs.get(si) else { return };
        let Some(ana) = song.analysis.clone() else { return };
        let view = self.project.view.clone();
        let dur = song.duration();
        let visible: Vec<usize> = (0..song.tracks.len()).filter(|&i| !song.tracks[i].hidden).collect();
        let h_ruler = 26.0;
        let h_sec = view.sec_h;
        let h_en = if view.show_energy { view.energy_h } else { 0.0 };
        let h_tr = view.track_h;
        let total_h = h_ruler + h_sec + h_en + h_tr * visible.len() as f32 + 4.0;
        let overflow = total_h + 40.0 > view_h;
        let width = ui.available_width();
        let (resp, painter) = ui.allocate_painter(vec2(width, total_h), Sense::click_and_drag());
        let rect = resp.rect;
        let gutter = 110.0;
        let x0 = rect.left() + gutter;
        let w = (rect.width() - gutter).max(10.0);
        let pps = self.ed.pps;
        let now = self.now();
        let playing = self.is_playing();
        let mut start = self.ed.start;
        let view_len = w as f64 / pps as f64;
        if self.ed.follow && playing && (now > start + view_len * 0.9 || now < start) {
            start = (now - view_len * 0.1).max(0.0);
        }
        start = start.clamp(0.0, (dur - view_len * 0.5).max(0.0));
        let tx = |t: f64| x0 + ((t - start) * pps as f64) as f32;
        let xt = |x: f32| start + ((x - x0) / pps) as f64;

        let y_r = rect.top();
        let y_sec = y_r + h_ruler;
        let y_en = y_sec + h_sec;
        let y_tr = y_en + h_en;
        let y_end = y_tr + h_tr * visible.len() as f32;
        let painter = painter.with_clip_rect(rect);
        painter.rect_filled(rect, 3.0, Color32::from_rgb(14, 14, 19));
        let content = Rect::from_min_max(pos2(x0, rect.top()), pos2(rect.right(), y_end));
        let pc = painter.with_clip_rect(content);
        let lab = |y: f32, h: f32, t: &str, c: Color32| {
            painter.text(pos2(rect.left() + 6.0, y + h / 2.0), egui::Align2::LEFT_CENTER, t, egui::FontId::proportional((h * 0.5).clamp(9.5, 13.0)), c);
        };
        let gray = Color32::from_rgb(150, 150, 165);
        lab(y_r, h_ruler, ts("Battute"), gray);
        lab(y_sec, h_sec, ts("Parti"), gray);
        if h_en > 0.0 {
            lab(y_en, h_en, ts("Energia"), gray);
        }
        for (row, &ti) in visible.iter().enumerate() {
            let t = &song.tracks[ti];
            let name: String = t.name.chars().take(15).collect();
            lab(y_tr + h_tr * row as f32, h_tr, &name, lanes::role_color(t.role));
        }
        lanes::draw_grid(&pc, &ana, start, view_len, pps, &tx, y_r + 2.0, y_r + 12.0, y_end);
        // lane borders (draggable to resize)
        let mut bottoms: Vec<(Lane, f32, usize)> = vec![(Lane::Sections, y_en, 0)];
        if h_en > 0.0 {
            bottoms.push((Lane::Energy, y_tr, 0));
        }
        for row in 0..visible.len() {
            bottoms.push((Lane::Track, y_tr + h_tr * (row + 1) as f32, row));
        }
        for (_, y, _) in &bottoms {
            painter.line_segment([pos2(rect.left(), *y), pos2(rect.right(), *y)], Stroke::new(0.6_f32, Color32::from_rgb(42, 42, 56)));
        }
        painter.line_segment([pos2(rect.left(), y_sec), pos2(rect.right(), y_sec)], Stroke::new(0.6_f32, Color32::from_rgb(42, 42, 56)));

        // anchors (first downbeat + extra downbeats)
        let anchors: Vec<(Option<usize>, f64)> = std::iter::once((None, ana.offset))
            .chain(ana.downbeats.iter().enumerate().map(|(i, &t)| (Some(i), t)))
            .collect();
        for (id, t) in &anchors {
            let x = tx(*t);
            let col = if id.is_none() { Color32::from_rgb(255, 220, 60) } else { Color32::from_rgb(255, 150, 40) };
            pc.add(egui::Shape::convex_polygon(
                vec![pos2(x, y_r + 12.0), pos2(x, y_r + 25.0), pos2(x + 11.0, y_r + 18.5)],
                col,
                Stroke::NONE,
            ));
            pc.line_segment([pos2(x, y_r + 12.0), pos2(x, y_end)], Stroke::new(1.2_f32, col.gamma_multiply(0.7)));
        }

        // sections
        for (i, s) in song.sections.iter().enumerate() {
            let r = Rect::from_min_max(pos2(tx(s.start), y_sec + 2.0), pos2(tx(s.end), y_en - 2.0));
            if r.right() < x0 || r.left() > rect.right() {
                continue;
            }
            let c = s.kind.color();
            pc.rect_filled(r, 3.0, Color32::from_rgba_unmultiplied(c[0], c[1], c[2], 175));
            let ir = Rect::from_min_max(pos2(r.left(), r.bottom() - 4.0), pos2(r.left() + r.width() * s.intensity, r.bottom()));
            pc.rect_filled(ir, 0.0, Color32::from_rgb(255, 230, 150));
            if self.ed.sel == Some(i) {
                pc.rect_stroke(r, 3.0, Stroke::new(2.0_f32, Color32::from_rgb(255, 210, 0)), StrokeKind::Inside);
            }
            let txt = if r.width() > 130.0 { format!("{} · {}", s.label, intensity_label(s.intensity)) } else { s.label.clone() };
            let p2 = pc.with_clip_rect(r.intersect(content));
            p2.text(pos2(r.left().max(x0) + 5.0, r.center().y - 2.0), egui::Align2::LEFT_CENTER, txt, egui::FontId::proportional(12.5), Color32::WHITE);
            // boundary handle
            if i > 0 {
                let x = tx(s.start);
                pc.rect_filled(Rect::from_center_size(pos2(x, r.center().y), vec2(4.0, (r.height() * 0.6).max(8.0))), 1.0, Color32::from_rgb(240, 240, 250));
            }
        }
        // energy
        if h_en > 0.0 {
            let mut x = x0;
            while x < rect.right() {
                let t = xt(x);
                if t >= 0.0 && t <= dur {
                    let v = ana.env.get((t * ENV_FPS) as usize).copied().unwrap_or(0.0);
                    let hh = v * (h_en - 6.0);
                    let col = Color32::from_rgb((60.0 + 190.0 * v) as u8, (90.0 + 60.0 * v) as u8, (200.0 - 120.0 * v) as u8);
                    pc.line_segment([pos2(x, y_tr - 2.0), pos2(x, y_tr - 2.0 - hh)], Stroke::new(2.0_f32, col));
                }
                x += 2.0;
            }
            for &(t, d) in &ana.stops {
                pc.rect_filled(Rect::from_min_max(pos2(tx(t), y_en + 2.0), pos2(tx(t + d), y_tr - 2.0)), 0.0, Color32::from_rgba_unmultiplied(255, 255, 255, 20));
            }
        }
        // waveforms
        let waves = self.waves.get(&si);
        for (row, &ti) in visible.iter().enumerate() {
            let t = &song.tracks[ti];
            let peaks = waves.and_then(|w| w.get(ti)).map(|v| v.as_slice());
            lanes::draw_wave(&pc, x0, rect.right(), &xt, dur, y_tr + h_tr * row as f32, h_tr, peaks, &t.env, lanes::role_color(t.role));
        }
        // playhead
        let px = tx(now);
        pc.line_segment([pos2(px, rect.top()), pos2(px, y_end)], Stroke::new(1.5_f32, Color32::from_rgb(255, 70, 70)));

        // ---------------- interaction
        let mut acts: Vec<EAct> = vec![];
        let pointer = resp.interact_pointer_pos().or(resp.hover_pos());
        let pressed = resp.is_pointer_button_down_on() && ui.input(|i| i.pointer.primary_pressed());
        let near_anchor = |p: egui::Pos2| -> Option<Option<usize>> {
            anchors.iter().filter(|(_, t)| (tx(*t) - p.x).abs() < 8.0).min_by(|a, b| (tx(a.1) - p.x).abs().total_cmp(&(tx(b.1) - p.x).abs())).map(|a| a.0)
        };
        let near_boundary = |p: egui::Pos2| -> Option<usize> { song.sections.iter().enumerate().skip(1).find(|(_, s)| (tx(s.start) - p.x).abs() < 6.0).map(|x| x.0) };
        let near_bottom = |p: egui::Pos2| -> Option<(Lane, f32, usize)> { bottoms.iter().find(|(_, y, _)| (p.y - y).abs() < 4.0).copied() };
        if resp.hovered() {
            let (scroll, zoom, alt, raw) = ui.input(|i| (i.smooth_scroll_delta, i.zoom_delta(), i.modifiers.alt, i.raw_scroll_delta));
            if alt && (raw.y.abs() + raw.x.abs()) > 0.0 {
                let d = if raw.y.abs() > raw.x.abs() { raw.y } else { raw.x };
                acts.push(EAct::TrackZoom((h_tr * (1.0 + d * 0.004)).clamp(8.0, 160.0)));
            } else if (zoom - 1.0).abs() > 1e-3 {
                if let Some(p) = pointer {
                    let anchor = xt(p.x);
                    let npps = (pps * zoom).clamp(3.0, 300.0);
                    acts.push(EAct::View((anchor - ((p.x - x0) / npps) as f64).max(0.0), npps));
                }
            } else {
                let d = if overflow { scroll.x } else if scroll.x.abs() > scroll.y.abs() { scroll.x } else { scroll.y };
                if d.abs() > 0.0 {
                    acts.push(EAct::View((start - (d / pps) as f64).max(0.0), pps));
                }
            }
            if let Some(p) = resp.hover_pos() {
                if near_bottom(p).is_some() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeVertical);
                } else if (p.y < y_sec && near_anchor(p).is_some()) || (p.y >= y_sec && p.y < y_en && near_boundary(p).is_some()) {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
                }
            }
        }
        if let Some(p) = pointer {
            let t = xt(p.x);
            if pressed {
                if let Some((lane, _, row)) = near_bottom(p) {
                    let h = *lanes::lane_height(&mut self.project.view.clone(), lane);
                    acts.push(EAct::Start(EdDrag::Resize(lane, p.y, h, row)));
                } else if p.x >= x0 {
                    if p.y < y_sec {
                        match near_anchor(p) {
                            Some(id) => acts.push(EAct::Start(EdDrag::Anchor(id))),
                            None => {
                                acts.push(EAct::Seek(t));
                                acts.push(EAct::Start(EdDrag::Seek));
                            }
                        }
                    } else if p.y < y_en {
                        if let Some(i) = near_boundary(p) {
                            acts.push(EAct::Select(Some(i)));
                            acts.push(EAct::Start(EdDrag::Boundary(i)));
                        } else {
                            acts.push(EAct::Select(song.section_at(t)));
                        }
                    } else {
                        acts.push(EAct::Seek(t));
                        acts.push(EAct::Start(EdDrag::Seek));
                    }
                }
            }
            if resp.dragged() && self.ed.drag.is_some() {
                acts.push(EAct::DragTo(t, p.y, ui.input(|i| i.modifiers.alt)));
            }
            if resp.double_clicked() && p.x >= x0 {
                if p.y < y_sec {
                    acts.push(EAct::AddDownbeat(ana.snap(t, 1.0)));
                } else if p.y < y_en && near_boundary(p).is_none() {
                    acts.push(EAct::Split(t));
                }
            }
            if resp.secondary_clicked() && p.x >= x0 {
                if p.y < y_sec {
                    if let Some(Some(i)) = near_anchor(p) {
                        acts.push(EAct::DelDownbeat(i));
                    }
                } else if p.y < y_en {
                    if let Some(i) = near_boundary(p) {
                        acts.push(EAct::Merge(i - 1));
                    }
                }
            }
        }
        if resp.drag_stopped() || (self.ed.drag.is_some() && !ui.input(|i| i.pointer.primary_down())) {
            acts.push(EAct::End);
        }
        if let Some(p) = resp.hover_pos() {
            if p.x >= x0 {
                let t = xt(p.x);
                resp.clone().on_hover_text_at_pointer(crate::i18n::t(&format!("{}  ·  battuta {}", fmt_time(t), ana.bar_beat(t))));
            }
        }
        self.ed.start = start;

        // overview
        let (ov, ovp) = ui.allocate_painter(vec2(width, 10.0), Sense::click_and_drag());
        ovp.rect_filled(ov.rect, 2.0, Color32::from_rgb(30, 30, 40));
        let ow = ov.rect.width() - gutter;
        for s in &self.project.songs[si].sections {
            let c = s.kind.color();
            let r = Rect::from_min_max(
                pos2(ov.rect.left() + gutter + (s.start / dur) as f32 * ow, ov.rect.top() + 1.0),
                pos2(ov.rect.left() + gutter + (s.end / dur) as f32 * ow, ov.rect.bottom() - 1.0),
            );
            ovp.rect_filled(r, 0.0, Color32::from_rgb(c[0], c[1], c[2]).gamma_multiply(0.7));
        }
        let vr = Rect::from_min_max(
            pos2(ov.rect.left() + gutter + (start / dur) as f32 * ow, ov.rect.top()),
            pos2(ov.rect.left() + gutter + (((start + view_len) / dur) as f32 * ow).min(ow), ov.rect.bottom()),
        );
        ovp.rect_stroke(vr, 1.0, Stroke::new(1.0_f32, Color32::WHITE), StrokeKind::Inside);
        if ov.dragged() || ov.clicked() {
            if let Some(p) = ov.interact_pointer_pos() {
                let t = ((p.x - ov.rect.left() - gutter) / ow) as f64 * dur;
                acts.push(EAct::View((t - view_len / 2.0).max(0.0), pps));
            }
        }
        self.apply_eacts(acts, &ana);
    }

    fn apply_eacts(&mut self, acts: Vec<EAct>, ana: &Analysis) {
        for a in acts {
            match a {
                EAct::Seek(t) => self.seek(t),
                EAct::Select(s) => self.ed.sel = s,
                EAct::View(s, p) => {
                    self.ed.start = s;
                    self.ed.pps = p;
                }
                EAct::TrackZoom(h) => self.project.view.track_h = h,
                EAct::Start(d) => {
                    if matches!(d, EdDrag::Anchor(_) | EdDrag::Boundary(_)) {
                        self.push_undo();
                    }
                    self.ed.drag = Some(d);
                }
                EAct::DragTo(t, y, alt) => {
                    let Some(d) = self.ed.drag else { continue };
                    match d {
                        EdDrag::Seek => self.seek(t),
                        EdDrag::Resize(lane, y0, h0, row) => {
                            let (lo, hi) = lanes::lane_limits(lane);
                            let k = if lane == Lane::Track { (row + 1) as f32 } else { 1.0 };
                            *lanes::lane_height(&mut self.project.view, lane) = (h0 + (y - y0) / k).clamp(lo, hi);
                        }
                        EdDrag::Anchor(id) => {
                            let Some(a) = self.cur_song_mut().and_then(|s| s.analysis.as_mut()) else { continue };
                            // magnet: stick to a nearby kick drum hit (Alt = free)
                            let mut tt = t.max(0.0);
                            if !alt {
                                if let Some(k) = a.kicks.iter().copied().filter(|k| (k - tt).abs() < 0.05).min_by(|x, y| (x - tt).abs().total_cmp(&(y - tt).abs())) {
                                    tt = k;
                                }
                            }
                            match id {
                                None => a.offset = tt,
                                Some(i) => {
                                    if let Some(x) = a.downbeats.get_mut(i) {
                                        *x = tt;
                                    }
                                }
                            }
                        }
                        EdDrag::Boundary(i) => {
                            let Some(s) = self.cur_song_mut() else { continue };
                            if i >= 1 && i < s.sections.len() {
                                let bar = ana.bar_len().max(0.1);
                                let lo = s.sections[i - 1].start + bar * 0.5;
                                let hi = s.sections[i].end - bar * 0.5;
                                let nt = ana.snap_bar(t).clamp(lo, hi.max(lo));
                                s.sections[i - 1].end = nt;
                                s.sections[i].start = nt;
                            }
                        }
                    }
                }
                EAct::End => {
                    if let Some(EdDrag::Anchor(Some(_))) = self.ed.drag {
                        if let Some(a) = self.cur_song_mut().and_then(|s| s.analysis.as_mut()) {
                            a.downbeats.sort_by(|x, y| x.total_cmp(y));
                        }
                    }
                    self.ed.drag = None;
                }
                EAct::AddDownbeat(t) => {
                    self.push_undo();
                    if let Some(a) = self.cur_song_mut().and_then(|s| s.analysis.as_mut()) {
                        if t > a.offset + 0.05 && !a.downbeats.iter().any(|d| (d - t).abs() < 0.05) {
                            a.downbeats.push(t);
                            a.downbeats.sort_by(|x, y| x.total_cmp(y));
                        }
                    }
                    self.status = ts("Nuovo battere aggiunto: da qui le battute ripartono (tasto destro sulla bandierina per toglierlo)").into();
                }
                EAct::DelDownbeat(i) => {
                    self.push_undo();
                    if let Some(a) = self.cur_song_mut().and_then(|s| s.analysis.as_mut()) {
                        if i < a.downbeats.len() {
                            a.downbeats.remove(i);
                        }
                    }
                }
                EAct::Split(t) => self.section_action(crate::ui_songs::SecAct::SplitAt(t)),
                EAct::Merge(i) => self.section_action(crate::ui_songs::SecAct::Merge(i)),
            }
        }
    }

}
