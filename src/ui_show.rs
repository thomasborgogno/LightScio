//! "Show" tab: transport, stage preview, timeline editor and inspector.

use crate::app::*;
#[allow(unused_imports)]
use crate::i18n::{t, ts};
use crate::autoshow;
use crate::lanes::{self, role_color, Lane};
use crate::model::*;
use crate::stage;
use egui::{pos2, vec2, Color32, Rect, RichText, Sense, Stroke, StrokeKind};

const RATES: [(f32, &str); 7] = [
    (0.25, "1/4 di battito"),
    (0.5, "1/2 battito"),
    (1.0, "1 battito"),
    (2.0, "2 battiti"),
    (4.0, "1 battuta"),
    (8.0, "2 battute"),
    (16.0, "4 battute"),
];
const MOVE_RATES: [(f32, &str); 6] =
    [(1.0, "1 battito"), (2.0, "2 battiti"), (4.0, "1 battuta"), (8.0, "2 battute"), (16.0, "4 battute"), (32.0, "8 battute")];
const SWATCHES: [[u8; 3]; 12] = [
    [255, 0, 0],
    [255, 90, 0],
    [255, 180, 0],
    [255, 255, 255],
    [255, 214, 170],
    [0, 255, 60],
    [0, 255, 200],
    [0, 160, 255],
    [0, 40, 255],
    [120, 0, 255],
    [220, 0, 255],
    [255, 0, 120],
];

pub fn c32(c: [u8; 3]) -> Color32 {
    Color32::from_rgb(c[0], c[1], c[2])
}


enum Act {
    Seek(f64),
    Select(Sel),
    StartDrag(TlDrag, f64),
    TrackZoom(f32),
    DragTo(f64),
    EndDrag,
    AddCue(f64),
    AddHit(f64),
    DelCue(usize),
    DelHit(usize),
    View(f64, f32),
}

fn rate_combo(ui: &mut egui::Ui, id: impl std::hash::Hash, v: &mut f32, opts: &[(f32, &'static str)]) {
    let cur = opts.iter().min_by(|a, b| (a.0 - *v).abs().total_cmp(&(b.0 - *v).abs())).map(|x| x.1).unwrap_or("");
    egui::ComboBox::from_id_salt(id).selected_text(ts(cur)).width(110.0).show_ui(ui, |ui| {
        for (r, l) in opts {
            ui.selectable_value(v, *r, ts(l));
        }
    });
}

impl App {
    pub fn ui_show(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("transport").show(ctx, |ui| self.transport_bar(ui));
        egui::SidePanel::right("inspector").default_width(340.0).min_width(280.0).resizable(true).show(ctx, |ui| {
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| self.inspector(ui));
        });
        egui::CentralPanel::default().show(ctx, |ui| {
            if self.project.songs.is_empty() {
                ui.centered_and_justified(|ui| {
                    ui.label(RichText::new(ts("Nessun brano. Vai su «Brani e analisi» e carica le tracce MP3 di una canzone.")).size(16.0));
                });
                return;
            }
            let avail = ui.available_size();
            let stage_h = (avail.y * self.project.view.stage_frac).clamp(80.0, (avail.y - 160.0).max(80.0));
            let t = self.now();
            if let Some(id) = stage::draw_perspective(ui, vec2(avail.x, stage_h), &self.project, &self.catalog, &self.states, self.sel_fixture, t) {
                self.sel_fixture = Some(id);
                self.sel_set = [id].into_iter().collect();
            }
            // draggable divider between stage and timeline
            let (sp, spp) = ui.allocate_painter(vec2(avail.x, 8.0), Sense::drag());
            let hov = sp.hovered() || sp.dragged();
            spp.rect_filled(
                egui::Rect::from_center_size(sp.rect.center(), vec2(60.0, 3.0)),
                2.0,
                if hov { Color32::from_rgb(255, 200, 90) } else { Color32::from_rgb(80, 80, 95) },
            );
            if hov {
                ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeVertical);
            }
            if sp.dragged() {
                self.project.view.stage_frac = ((stage_h + sp.drag_delta().y) / avail.y.max(1.0)).clamp(0.1, 0.85);
            }
            self.timeline_controls(ui);
            let view_h = ui.available_height();
            egui::ScrollArea::vertical().id_salt("tl_scroll").auto_shrink([false, false]).show(ui, |ui| self.timeline(ui, view_h));
        });
        // keyboard
        let no_focus = ctx.memory(|m| m.focused().is_none());
        if no_focus {
            let (space, del, undo) = ctx.input(|i| {
                (
                    i.key_pressed(egui::Key::Space),
                    i.key_pressed(egui::Key::Delete) || i.key_pressed(egui::Key::Backspace),
                    i.modifiers.command && i.key_pressed(egui::Key::Z),
                )
            });
            if space {
                if self.is_playing() {
                    self.pause()
                } else {
                    self.play()
                }
            }
            if del {
                match self.sel {
                    Sel::Cue(i) => self.delete_cue(i),
                    Sel::Hit(i) if self.tl.hit_mode => self.delete_hit(i),
                    _ => {}
                }
            }
            if undo {
                self.do_undo();
            }
        }
    }

    fn delete_cue(&mut self, i: usize) {
        self.push_undo();
        if let Some(s) = self.cur_song_mut() {
            if i < s.cues.len() {
                s.cues.remove(i);
            }
        }
        self.sel = Sel::None;
    }
    fn delete_hit(&mut self, i: usize) {
        self.push_undo();
        if let Some(s) = self.cur_song_mut() {
            if i < s.hits.len() {
                s.hits.remove(i);
            }
        }
        self.sel = Sel::None;
    }

    fn transport_bar(&mut self, ui: &mut egui::Ui) {
        ui.add_space(3.0);
        ui.horizontal(|ui| {
            let names: Vec<String> = self.project.songs.iter().map(|s| format!("{:>3}  {}", s.program, s.name)).collect();
            let mut pick = self.song;
            egui::ComboBox::from_id_salt("song_pick")
                .width(230.0)
                .selected_text(names.get(self.song).cloned().unwrap_or_else(|| "—".into()))
                .show_ui(ui, |ui| {
                    for (i, n) in names.iter().enumerate() {
                        ui.selectable_value(&mut pick, i, n);
                    }
                });
            if pick != self.song {
                self.select_song(pick);
            }
            ui.separator();
            let big = |s: &str| RichText::new(s).size(18.0);
            if ui.button(big("⏮")).on_hover_text(ts("Torna all'inizio")).clicked() {
                self.seek(0.0);
                self.tl.start = 0.0;
            }
            let playing = self.is_playing();
            if ui.button(big(if playing { "⏸" } else { "▶" })).on_hover_text(ts("Play / pausa (barra spaziatrice)")).clicked() {
                if playing {
                    self.pause()
                } else {
                    self.play()
                }
            }
            if ui.button(big("⏹")).on_hover_text(ts("Stop")).clicked() {
                self.stop();
            }
            let t = self.now();
            let (bb, bpm, sec) = match self.cur_song() {
                Some(s) => (
                    s.analysis.as_ref().map(|a| a.bar_beat(t)).unwrap_or_default(),
                    s.analysis.as_ref().map(|a| a.bpm).unwrap_or(0.0),
                    s.section_at(t).map(|i| s.sections[i].label.clone()).unwrap_or_default(),
                ),
                None => (String::new(), 0.0, String::new()),
            };
            ui.label(RichText::new(fmt_time(t)).monospace().size(20.0).color(Color32::from_rgb(255, 210, 90)));
            ui.label(RichText::new(crate::i18n::t(&format!("battuta {bb}"))).monospace());
            ui.label(RichText::new(crate::i18n::t(&format!("{bpm:.1} BPM"))).monospace().color(Color32::GRAY));
            if !sec.is_empty() {
                ui.label(RichText::new(sec).strong());
            }
            ui.separator();
            if self.audio_for == Some(self.song) {
                let mut v = self.player.volume();
                ui.label("🔊");
                if ui.add(egui::Slider::new(&mut v, 0.0..=1.0).show_value(false)).changed() {
                    self.player.set_volume(v);
                }
            } else if self.cur_song().map(|s| !s.tracks.is_empty()).unwrap_or(false) {
                if self.job_running("Carico") || self.job_running("Analisi") {
                    ui.spinner();
                } else if !self.cur_song().map(crate::relink::song_has_audio).unwrap_or(false) {
                    if ui
                        .button(ts("🔗 Ricollega tracce audio…"))
                        .on_hover_text(ts("Indica la nuova cartella degli MP3 dopo aver spostato il progetto su un altro computer"))
                        .clicked()
                    {
                        self.relink_dialog();
                    }
                } else if ui.button(ts("🔈 Carica audio per l'anteprima")).clicked() {
                    self.start_load(self.song, false);
                }
            } else {
                ui.label(RichText::new(ts("anteprima senza audio")).color(Color32::GRAY));
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button(ts("💾 Esporta MIDI…")).on_hover_text(ts("Salva la traccia MIDI di controllo da caricare nel player delle basi")).clicked() {
                    self.export_midi(false);
                }
                ui.menu_button(ts("✨ Genera show"), |ui| {
                    if ui.button(ts("Con le regole automatiche")).clicked() {
                        self.regenerate_rules();
                        ui.close();
                    }
                    let busy = self.job_running("Claude");
                    if ui.add_enabled(!busy, egui::Button::new(ts("Con Claude Code (installato sul PC)"))).clicked() {
                        self.start_claude(self.song);
                        ui.close();
                    }
                });
                if ui.add_enabled(!self.undo.is_empty(), egui::Button::new(ts("↶ Annulla"))).clicked() {
                    self.do_undo();
                }
            });
        });
        ui.add_space(3.0);
    }

    pub fn regenerate_rules(&mut self) {
        if self.cur_song().and_then(|s| s.analysis.as_ref()).is_none() {
            self.status = ts("Analizza prima il brano").into();
            return;
        }
        self.push_undo();
        if let Some(s) = self.cur_song_mut() {
            let (c, h) = autoshow::generate(s);
            s.cues = c;
            s.hits = h;
        }
        self.sel = Sel::None;
        self.status = ts("Show rigenerato con le regole automatiche (Annulla per tornare indietro)").into();
    }

    fn timeline_controls(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(ts("Zoom ↔"));
            ui.add(egui::Slider::new(&mut self.tl.pps, 3.0..=250.0).logarithmic(true).show_value(false));
            ui.label(ts("Zoom ↕ tracce"));
            ui.add(egui::Slider::new(&mut self.project.view.track_h, 8.0..=160.0).show_value(false))
                .on_hover_text(ts("Altezza delle tracce (anche Alt + rotellina sulla timeline)"));
            let si = self.song;
            ui.menu_button(ts("👁 Tracce"), |ui| {
                ui.label(RichText::new(ts("Cosa mostrare (vale per tutto il progetto)")).strong());
                ui.checkbox(&mut self.project.view.show_energy, ts("Energia"));
                if let Some(s) = self.project.songs.get_mut(si) {
                    for t in s.tracks.iter_mut() {
                        let mut vis = !t.hidden;
                        if ui.checkbox(&mut vis, RichText::new(&t.name).color(role_color(t.role))).changed() {
                            t.hidden = !vis;
                        }
                    }
                    ui.separator();
                    ui.horizontal(|ui| {
                        if ui.button(ts("Mostra tutte")).clicked() {
                            s.tracks.iter_mut().for_each(|t| t.hidden = false);
                        }
                        if ui.button(ts("Nascondi tutte")).clicked() {
                            s.tracks.iter_mut().for_each(|t| t.hidden = true);
                        }
                    });
                }
                ui.separator();
                ui.label(RichText::new(ts("Per ridimensionare una corsia trascina la sua riga inferiore.")).size(11.0).color(Color32::GRAY));
                if ui.button(ts("Altezze predefinite")).clicked() {
                    let keep = (self.project.view.show_energy, self.project.view.stage_frac);
                    self.project.view = ViewSettings::default();
                    self.project.view.show_energy = keep.0;
                    self.project.view.stage_frac = keep.1;
                }
            });
            ui.checkbox(&mut self.tl.snap, ts("Aggancia ai battiti"));
            ui.checkbox(&mut self.tl.follow, ts("Segui"));
            ui.separator();
            let txt = if self.tl.hit_mode { RichText::new(ts("✚ Modifica colpi: ATTIVA")).strong().color(Color32::from_rgb(255, 200, 90)) } else { RichText::new(ts("✚ Modifica colpi")) };
            ui.toggle_value(&mut self.tl.hit_mode, txt)
                .on_hover_text(ts("Quando è attiva: clic sulla corsia Colpi = nuovo colpo, trascina per spostare, tasto destro o Canc per eliminare"));
            ui.add_enabled_ui(self.tl.hit_mode, |ui| {
                egui::ComboBox::from_id_salt("hit_kind").selected_text(self.tl.add_hit.label()).show_ui(ui, |ui| {
                    for k in HitKind::ALL {
                        ui.selectable_value(&mut self.tl.add_hit, k, k.label());
                    }
                });
            });
        });
    }

    fn timeline(&mut self, ui: &mut egui::Ui, view_h: f32) {
        let Some(song) = self.project.songs.get(self.song) else { return };
        let ana = song.analysis.clone().unwrap_or(Analysis { bpm: 120.0, beats_per_bar: 4, duration: song.duration(), ..Default::default() });
        let dur = song.duration();
        let view = self.project.view.clone();
        let visible: Vec<usize> = (0..song.tracks.len()).filter(|&i| !song.tracks[i].hidden).collect();
        let ntr = visible.len();
        let h_ruler = 18.0;
        let h_sec = view.sec_h;
        let h_cue = view.cue_h;
        let h_hit = view.hit_h;
        let h_en = if view.show_energy { view.energy_h } else { 0.0 };
        let h_tr = view.track_h;
        let total_h = h_ruler + h_sec + h_en + h_tr * ntr as f32 + h_cue + h_hit + 6.0;
        let overflow = total_h + 60.0 > view_h;
        let width = ui.available_width();
        let (resp, painter) = ui.allocate_painter(vec2(width, total_h), Sense::click_and_drag());
        let rect = resp.rect;
        let gutter = 104.0;
        let x0 = rect.left() + gutter;
        let w = (rect.width() - gutter).max(10.0);
        let pps = self.tl.pps;
        let now = self.now();
        let playing = self.is_playing();

        // view follow / clamp
        let mut start = self.tl.start;
        let view_len = w as f64 / pps as f64;
        if self.tl.follow && playing && (now > start + view_len * 0.9 || now < start) {
            start = (now - view_len * 0.1).max(0.0);
        }
        start = start.clamp(0.0, (dur - view_len * 0.5).max(0.0));
        let tx = |t: f64| x0 + ((t - start) * pps as f64) as f32;
        let xt = |x: f32| start + ((x - x0) / pps) as f64;

        // lanes
        let y_ruler = rect.top();
        let y_sec = y_ruler + h_ruler;
        let y_cue = y_sec + h_sec;
        let y_hit = y_cue + h_cue;
        let y_en = y_hit + h_hit;
        let y_tr = y_en + h_en;
        let y_end = y_tr + h_tr * ntr as f32;
        let painter = painter.with_clip_rect(rect);
        painter.rect_filled(rect, 3.0, Color32::from_rgb(14, 14, 19));
        if self.tl.hit_mode {
            painter.rect_filled(Rect::from_min_max(pos2(rect.left(), y_hit), pos2(rect.right(), y_en)), 0.0, Color32::from_rgb(45, 36, 14));
        }
        let lane_label = |y: f32, h: f32, txt: &str, col: Color32| {
            painter.text(pos2(rect.left() + 6.0, y + h / 2.0), egui::Align2::LEFT_CENTER, txt, egui::FontId::proportional((h * 0.45).clamp(9.5, 13.0)), col);
        };
        let gray = Color32::from_rgb(150, 150, 165);
        lane_label(y_sec, h_sec, ts("Parti"), gray);
        if h_en > 0.0 {
            lane_label(y_en, h_en, ts("Energia"), gray);
        }
        for (row, &ti) in visible.iter().enumerate() {
            let t = &song.tracks[ti];
            let name: String = t.name.chars().take(14).collect();
            lane_label(y_tr + h_tr * row as f32, h_tr, &name, role_color(t.role));
        }
        lane_label(y_cue, h_cue, ts("Scene (cue)"), gray);
        lane_label(y_hit, h_hit, if self.tl.hit_mode { ts("Colpi ✚") } else { ts("Colpi") }, gray);
        let content = Rect::from_min_max(pos2(x0, rect.top()), pos2(rect.right(), y_end));
        let painter_c = painter.with_clip_rect(content);
        // lane borders: drag them to resize each kind of lane
        let mut bottoms: Vec<(Lane, f32, usize)> = vec![(Lane::Sections, y_cue, 0), (Lane::Cues, y_hit, 0), (Lane::Hits, y_en, 0)];
        if h_en > 0.0 {
            bottoms.push((Lane::Energy, y_tr, 0));
        }
        for row in 0..ntr {
            bottoms.push((Lane::Track, y_tr + h_tr * (row + 1) as f32, row));
        }
        painter.line_segment([pos2(rect.left(), y_sec), pos2(rect.right(), y_sec)], Stroke::new(0.5_f32, Color32::from_rgb(40, 40, 52)));
        for (_, y, _) in &bottoms {
            painter.line_segment([pos2(rect.left(), *y), pos2(rect.right(), *y)], Stroke::new(0.6_f32, Color32::from_rgb(42, 42, 56)));
        }
        let near_bottom = |p: egui::Pos2| bottoms.iter().find(|(_, y, _)| (p.y - y).abs() < 4.0).copied();
        let mut resize_press = false;
        if let Some(p) = resp.hover_pos() {
            if near_bottom(p).is_some() || matches!(self.tl.drag, Some(TlDrag::Resize(..))) {
                ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeVertical);
            }
        }
        if resp.is_pointer_button_down_on() && ui.input(|i| i.pointer.primary_pressed()) {
            if let Some(p) = resp.interact_pointer_pos() {
                if let Some((lane, _, row)) = near_bottom(p) {
                    let h = *lanes::lane_height(&mut self.project.view, lane);
                    self.tl.drag = Some(TlDrag::Resize(lane, p.y, h, row));
                    resize_press = true;
                }
            }
        }
        if let Some(TlDrag::Resize(lane, y0, h0, row)) = self.tl.drag {
            if let Some(p) = resp.interact_pointer_pos() {
                let (lo, hi) = lanes::lane_limits(lane);
                let k = if lane == Lane::Track { (row + 1) as f32 } else { 1.0 };
                *lanes::lane_height(&mut self.project.view, lane) = (h0 + (p.y - y0) / k).clamp(lo, hi);
            }
        }

        // grid
        lanes::draw_grid(&painter_c, &ana, start, view_len, pps, &tx, y_ruler + 1.0, y_ruler + 10.0, y_end);

        // sections
        for (i, s) in song.sections.iter().enumerate() {
            let r = Rect::from_min_max(pos2(tx(s.start), y_sec + 2.0), pos2(tx(s.end), y_sec + h_sec - 2.0));
            if r.right() < x0 || r.left() > rect.right() {
                continue;
            }
            let c = s.kind.color();
            painter_c.rect_filled(r, 3.0, Color32::from_rgba_unmultiplied(c[0], c[1], c[2], 170));
            // intensity bar
            let ir = Rect::from_min_max(pos2(r.left(), r.bottom() - 4.0), pos2(r.left() + r.width() * s.intensity, r.bottom()));
            painter_c.rect_filled(ir, 0.0, Color32::from_rgb(255, 230, 150));
            if self.sel == Sel::Section(i) {
                painter_c.rect_stroke(r, 3.0, Stroke::new(2.0_f32, Color32::from_rgb(255, 210, 0)), StrokeKind::Inside);
            }
            let txt = if r.width() > 120.0 { format!("{} · {}", s.label, intensity_label(s.intensity)) } else { s.label.clone() };
            let pc = painter_c.with_clip_rect(r.intersect(content));
            pc.text(pos2(r.left().max(x0) + 5.0, r.center().y - 2.0), egui::Align2::LEFT_CENTER, txt, egui::FontId::proportional(12.0), Color32::WHITE);
        }

        // energy + drum hits
        if h_en > 0.0 {
            let env = &ana.env;
            let mut x = x0;
            while x < rect.right() {
                let t = xt(x);
                if t >= 0.0 && t <= dur {
                    let k = (t * ENV_FPS) as usize;
                    let v = env.get(k).copied().unwrap_or(0.0);
                    let h = v * (h_en - 6.0);
                    let col = Color32::from_rgb((60.0 + 190.0 * v) as u8, (90.0 + 60.0 * v) as u8, (200.0 - 120.0 * v) as u8);
                    painter_c.line_segment([pos2(x, y_en + h_en - 2.0), pos2(x, y_en + h_en - 2.0 - h)], Stroke::new(2.0_f32, col));
                }
                x += 2.0;
            }
            if pps > 25.0 {
                let (a, z) = (start, start + view_len);
                for &k in ana.kicks.iter().filter(|&&k| k >= a && k <= z) {
                    painter_c.line_segment([pos2(tx(k), y_en + 2.0), pos2(tx(k), y_en + 8.0)], Stroke::new(1.5_f32, Color32::from_rgb(255, 140, 60)));
                }
                for &k in ana.snares.iter().filter(|&&k| k >= a && k <= z) {
                    painter_c.line_segment([pos2(tx(k), y_en + 9.0), pos2(tx(k), y_en + 14.0)], Stroke::new(1.5_f32, Color32::from_rgb(120, 220, 255)));
                }
            }
            for &(t, _) in &ana.accents {
                let xx = tx(t);
                painter_c.add(egui::Shape::convex_polygon(
                    vec![pos2(xx - 4.0, y_en + 1.0), pos2(xx + 4.0, y_en + 1.0), pos2(xx, y_en + 8.0)],
                    Color32::from_rgb(255, 230, 120),
                    Stroke::NONE,
                ));
            }
            for &(t, d) in &ana.stops {
                let r = Rect::from_min_max(pos2(tx(t), y_en + 2.0), pos2(tx(t + d), y_en + h_en - 2.0));
                painter_c.rect_filled(r, 0.0, Color32::from_rgba_unmultiplied(255, 255, 255, 18));
            }
        }

        // audio tracks (waveforms)
        let waves = self.waves.get(&self.song);
        for (row, &ti) in visible.iter().enumerate() {
            let tr = &song.tracks[ti];
            let peaks = waves.and_then(|w| w.get(ti)).map(|v| v.as_slice());
            lanes::draw_wave(&painter_c, x0, rect.right(), &xt, dur, y_tr + h_tr * row as f32, h_tr, peaks, &tr.env, role_color(tr.role));
        }

        // cues
        let mut cue_boxes: Vec<(usize, Rect)> = vec![];
        for (i, c) in song.cues.iter().enumerate() {
            let x = tx(c.time);
            let next_x = song.cues.get(i + 1).map(|n| tx(n.time)).unwrap_or(tx(dur));
            if next_x < x0 || x > rect.right() {
                continue;
            }
            let col = c.looks.get(Group::BackWash.idx()).and_then(|l| l.colors.first()).copied().unwrap_or([200, 200, 200]);
            let r = Rect::from_min_max(pos2(x, y_cue + 3.0), pos2(next_x.max(x + 6.0), y_cue + h_cue - 3.0));
            painter_c.rect_filled(r, 2.0, Color32::from_rgba_unmultiplied(col[0] / 3 + 20, col[1] / 3 + 20, col[2] / 3 + 20, 255));
            painter_c.rect_filled(Rect::from_min_max(r.min, pos2(x + 4.0, r.max.y)), 1.0, c32(col));
            let selected = self.sel == Sel::Cue(i);
            if selected {
                painter_c.rect_stroke(r, 2.0, Stroke::new(2.0_f32, Color32::from_rgb(255, 210, 0)), StrokeKind::Inside);
            }
            painter_c.line_segment([pos2(x, y_hit), pos2(x, y_end)], Stroke::new(0.7_f32, Color32::from_rgba_unmultiplied(col[0], col[1], col[2], 70)));
            let pc = painter_c.with_clip_rect(r.intersect(content));
            pc.text(pos2(x.max(x0) + 7.0, r.center().y), egui::Align2::LEFT_CENTER, format!("{} {}", i, c.name), egui::FontId::proportional(11.0), Color32::WHITE);
            // the whole bar of the scene is clickable
            cue_boxes.push((i, r));
        }

        // hits
        let mut hit_boxes: Vec<(usize, Rect)> = vec![];
        for (i, h) in song.hits.iter().enumerate() {
            let x = tx(h.time);
            if x < x0 - 50.0 || x > rect.right() {
                continue;
            }
            let wpx = (h.duration as f32 * pps).max(6.0);
            let r = Rect::from_min_max(pos2(x - 3.0, y_hit + 4.0), pos2(x - 3.0 + wpx, y_hit + h_hit - 4.0));
            let c = h.kind.color();
            painter_c.rect_filled(r, 2.0, c32(c));
            if h.kind == HitKind::Blackout {
                painter_c.rect_stroke(r, 2.0, Stroke::new(1.0_f32, Color32::GRAY), StrokeKind::Inside);
            }
            if self.sel == Sel::Hit(i) {
                painter_c.rect_stroke(r.expand(2.0), 2.0, Stroke::new(2.0_f32, Color32::from_rgb(255, 210, 0)), StrokeKind::Inside);
            }
            hit_boxes.push((i, r.expand(2.0)));
        }

        // playhead
        let px = tx(now);
        painter_c.line_segment([pos2(px, rect.top()), pos2(px, y_end)], Stroke::new(1.5_f32, Color32::from_rgb(255, 70, 70)));

        // ------------------------------------------------ interaction
        let mut acts: Vec<Act> = vec![];
        let snap_t = |t: f64, div: f64| if self.tl.snap && song.analysis.is_some() { ana.snap(t, div) } else { t.max(0.0) };
        let pointer = resp.interact_pointer_pos().or(resp.hover_pos());
        let pressed = !resize_press && !matches!(self.tl.drag, Some(TlDrag::Resize(..))) && resp.is_pointer_button_down_on() && ui.input(|i| i.pointer.primary_pressed());
        if resp.hovered() {
            let (scroll, zoom, alt, raw) = ui.input(|i| (i.smooth_scroll_delta, i.zoom_delta(), i.modifiers.alt, i.raw_scroll_delta));
            if alt && (raw.y.abs() + raw.x.abs()) > 0.0 {
                let d = if raw.y.abs() > raw.x.abs() { raw.y } else { raw.x };
                acts.push(Act::TrackZoom((self.project.view.track_h * (1.0 + d * 0.004)).clamp(8.0, 80.0)));
            } else if (zoom - 1.0).abs() > 1e-3 {
                if let Some(p) = pointer {
                    let anchor = xt(p.x);
                    let npps = (pps * zoom).clamp(3.0, 250.0);
                    let nstart = anchor - ((p.x - x0) / npps) as f64;
                    acts.push(Act::View(nstart.max(0.0), npps));
                }
            } else {
                // when the lanes are taller than the view, the plain wheel scrolls
                // vertically (handled by the scroll area); horizontal wheel pans
                let d = if overflow { scroll.x } else if scroll.x.abs() > scroll.y.abs() { scroll.x } else { scroll.y };
                if d.abs() > 0.0 {
                    acts.push(Act::View((start - (d / pps) as f64).max(0.0), pps));
                }
            }
        }
        let hit_mode = self.tl.hit_mode;
        if let Some(p) = pointer {
            let in_content = p.x >= x0;
            if pressed && in_content {
                let t = xt(p.x);
                if p.y < y_sec || p.y >= y_en {
                    acts.push(Act::Seek(t));
                    acts.push(Act::StartDrag(TlDrag::Seek, 0.0));
                } else if p.y < y_cue {
                    // section boundary?
                    let mut done = false;
                    for (i, s) in song.sections.iter().enumerate().skip(1) {
                        if (tx(s.start) - p.x).abs() < 5.0 {
                            acts.push(Act::Select(Sel::Section(i)));
                            acts.push(Act::StartDrag(TlDrag::Boundary(i), 0.0));
                            done = true;
                            break;
                        }
                    }
                    if !done {
                        if let Some(i) = song.section_at(t) {
                            acts.push(Act::Select(Sel::Section(i)));
                        }
                    }
                } else if p.y < y_hit {
                    if let Some((i, _)) = cue_boxes.iter().rev().find(|(_, r)| r.contains(p)) {
                        acts.push(Act::Select(Sel::Cue(*i)));
                        // drag only from near the start of the scene, so clicks inside the bar just select
                        if (p.x - tx(song.cues[*i].time)).abs() < 14.0 {
                            acts.push(Act::StartDrag(TlDrag::Cue(*i), t - song.cues[*i].time));
                        }
                    }
                } else if p.y < y_en {
                    if let Some((i, _)) = hit_boxes.iter().rev().find(|(_, r)| r.contains(p)) {
                        acts.push(Act::Select(Sel::Hit(*i)));
                        if hit_mode {
                            acts.push(Act::StartDrag(TlDrag::Hit(*i), t - song.hits[*i].time));
                        }
                    }
                }
            }
            if resp.dragged() && self.tl.drag.is_some() && !matches!(self.tl.drag, Some(TlDrag::Resize(..))) {
                acts.push(Act::DragTo(xt(p.x)));
            }
            if resp.double_clicked() && in_content && p.y >= y_cue && p.y < y_hit && !cue_boxes.iter().any(|(_, r)| r.contains(p) && (p.x - r.left()).abs() < 14.0) {
                acts.push(Act::AddCue(snap_t(xt(p.x), 1.0)));
            }
            if hit_mode && resp.clicked() && in_content && p.y >= y_hit && p.y < y_en && !hit_boxes.iter().any(|(_, r)| r.contains(p)) {
                acts.push(Act::AddHit(snap_t(xt(p.x), 2.0)));
            }
            if resp.secondary_clicked() && in_content {
                if hit_mode && p.y >= y_hit && p.y < y_en {
                    if let Some((i, _)) = hit_boxes.iter().rev().find(|(_, r)| r.contains(p)) {
                        acts.push(Act::DelHit(*i));
                    }
                } else if p.y >= y_cue && p.y < y_hit {
                    if let Some((i, _)) = cue_boxes.iter().rev().find(|(_, r)| r.contains(p)) {
                        acts.push(Act::DelCue(*i));
                    }
                }
            }
        }
        if resp.drag_stopped() || (self.tl.drag.is_some() && !ui.input(|i| i.pointer.primary_down())) {
            acts.push(Act::EndDrag);
        }
        let hover_tip = pointer.filter(|p| p.x >= x0 && resp.hovered()).map(|p| (p, xt(p.x)));
        if let Some(p) = resp.hover_pos() {
            let near_cue = p.y >= y_cue && p.y < y_hit && cue_boxes.iter().any(|(_, r)| (p.x - r.left()).abs() < 14.0);
            let near_hit = hit_mode && p.y >= y_hit && p.y < y_en && hit_boxes.iter().any(|(_, r)| r.contains(p));
            let near_bound = p.y >= y_sec && p.y < y_cue && song.sections.iter().skip(1).any(|s| (tx(s.start) - p.x).abs() < 5.0);
            if near_cue || near_hit || near_bound {
                ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
            }
        }

        // overview strip
        let (ov, ovp) = ui.allocate_painter(vec2(width, 10.0), Sense::click_and_drag());
        ovp.rect_filled(ov.rect, 2.0, Color32::from_rgb(30, 30, 40));
        let ow = ov.rect.width() - gutter;
        for s in &song.sections {
            let c = s.kind.color();
            let r = Rect::from_min_max(
                pos2(ov.rect.left() + gutter + (s.start / dur) as f32 * ow, ov.rect.top() + 1.0),
                pos2(ov.rect.left() + gutter + (s.end / dur) as f32 * ow, ov.rect.bottom() - 1.0),
            );
            ovp.rect_filled(r, 0.0, c32(c).gamma_multiply(0.7));
        }
        let vr = Rect::from_min_max(
            pos2(ov.rect.left() + gutter + (start / dur) as f32 * ow, ov.rect.top()),
            pos2(ov.rect.left() + gutter + (((start + view_len) / dur) as f32 * ow).min(ow), ov.rect.bottom()),
        );
        ovp.rect_stroke(vr, 1.0, Stroke::new(1.0_f32, Color32::WHITE), StrokeKind::Inside);
        if ov.dragged() || ov.clicked() {
            if let Some(p) = ov.interact_pointer_pos() {
                let t = ((p.x - ov.rect.left() - gutter) / ow) as f64 * dur;
                acts.push(Act::View((t - view_len / 2.0).max(0.0), pps));
            }
        }
        ui.label(
            RichText::new(ts("Clic: posiziona la riproduzione · clic su una scena: selezionala · trascina l'inizio di una scena per spostarla · doppio clic: nuova scena · Colpi: attiva «Modifica colpi» per aggiungerli/spostarli · Ctrl+rotellina: zoom ↔ · Alt+rotellina: zoom ↕"))
                .size(10.5)
                .color(Color32::from_rgb(130, 130, 145)),
        );

        self.tl.start = start;
        if let Some((p, t)) = hover_tip {
            if p.y < y_hit + h_hit {
                resp.on_hover_text_at_pointer(crate::i18n::t(&format!("{}  ·  battuta {}", fmt_time(t), ana.bar_beat(t))));
            }
        }
        self.apply_acts(acts, &ana);
    }

    fn apply_acts(&mut self, acts: Vec<Act>, ana: &Analysis) {
        let snap = self.tl.snap && self.cur_song().map(|s| s.analysis.is_some()).unwrap_or(false);
        for a in acts {
            match a {
                Act::View(s, p) => {
                    self.tl.start = s;
                    self.tl.pps = p;
                }
                Act::Seek(t) => self.seek(t),
                Act::Select(s) => self.sel = s,
                Act::TrackZoom(h) => self.project.view.track_h = h,
                Act::StartDrag(d, grab) => {
                    self.tl.grab = grab;
                    if matches!(d, TlDrag::Cue(_) | TlDrag::Hit(_) | TlDrag::Boundary(_)) {
                        self.push_undo();
                    }
                    self.tl.drag = Some(d);
                }
                Act::DragTo(t) => {
                    let Some(d) = self.tl.drag else { continue };
                    if d == TlDrag::Seek {
                        self.seek(t);
                        continue;
                    }
                    let ana = ana.clone();
                    let grab = self.tl.grab;
                    let Some(s) = self.cur_song_mut() else { continue };
                    match d {
                        TlDrag::Cue(i) => {
                            if let Some(c) = s.cues.get_mut(i) {
                                let t = t - grab;
                                c.time = if snap { ana.snap(t, 1.0) } else { t.max(0.0) };
                            }
                        }
                        TlDrag::Hit(i) => {
                            if let Some(h) = s.hits.get_mut(i) {
                                let t = t - grab;
                                h.time = if snap { ana.snap(t, 4.0) } else { t.max(0.0) };
                            }
                        }
                        TlDrag::Boundary(i) => {
                            if i >= 1 && i < s.sections.len() {
                                let lo = s.sections[i - 1].start + 0.5;
                                let hi = s.sections[i].end - 0.5;
                                let nt = if snap { ana.snap(t, 1.0) } else { t }.clamp(lo, hi.max(lo));
                                s.sections[i - 1].end = nt;
                                s.sections[i].start = nt;
                            }
                        }
                        TlDrag::Seek | TlDrag::Resize(..) => {}
                    }
                }
                Act::EndDrag => {
                    if let Some(d) = self.tl.drag.take() {
                        let song = self.song;
                        if let Some(s) = self.project.songs.get_mut(song) {
                            match d {
                                TlDrag::Cue(i) => {
                                    let key = s.cues.get(i).map(|c| (c.time, c.name.clone()));
                                    s.cues.sort_by(|a, b| a.time.total_cmp(&b.time));
                                    if let Some(k) = key {
                                        if let Some(j) = s.cues.iter().position(|c| c.time == k.0 && c.name == k.1) {
                                            self.sel = Sel::Cue(j);
                                        }
                                    }
                                }
                                TlDrag::Hit(i) => {
                                    let key = s.hits.get(i).map(|h| (h.time, h.kind));
                                    s.hits.sort_by(|a, b| a.time.total_cmp(&b.time));
                                    if let Some(k) = key {
                                        if let Some(j) = s.hits.iter().position(|h| h.time == k.0 && h.kind == k.1) {
                                            self.sel = Sel::Hit(j);
                                        }
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                }
                Act::AddCue(t) => {
                    self.push_undo();
                    let Some(s) = self.cur_song_mut() else { continue };
                    let prev = s.cues.iter().rev().find(|c| c.time <= t).cloned();
                    let mut c = prev.unwrap_or_else(|| Cue::blank(t, ts("Scena")));
                    c.time = t;
                    c.name = crate::i18n::t(&format!("Scena {}", s.cues.len() + 1));
                    s.cues.push(c);
                    s.cues.sort_by(|a, b| a.time.total_cmp(&b.time));
                    let j = s.cues.iter().position(|c| c.time == t).unwrap_or(0);
                    self.sel = Sel::Cue(j);
                }
                Act::AddHit(t) => {
                    self.push_undo();
                    let kind = self.tl.add_hit;
                    let spb = ana.spb();
                    let Some(s) = self.cur_song_mut() else { continue };
                    let duration = match kind {
                        HitKind::Blinder => 0.2,
                        HitKind::Flash => 0.08,
                        HitKind::Strobe => spb * 2.0,
                        HitKind::Blackout => spb,
                    };
                    s.hits.push(Hit { time: t, kind, duration });
                    s.hits.sort_by(|a, b| a.time.total_cmp(&b.time));
                    let j = s.hits.iter().position(|h| h.time == t && h.kind == kind).unwrap_or(0);
                    self.sel = Sel::Hit(j);
                }
                Act::DelCue(i) => self.delete_cue(i),
                Act::DelHit(i) => self.delete_hit(i),
            }
        }
    }

    // ------------------------------------------------------------ inspector

    fn inspector(&mut self, ui: &mut egui::Ui) {
        match self.sel {
            Sel::Cue(i) => self.inspect_cue(ui, i),
            Sel::Section(i) => self.inspect_section(ui, i),
            Sel::Hit(i) => self.inspect_hit(ui, i),
            Sel::None => {
                ui.heading(ts("Come si usa"));
                ui.label(ts("• La riga «Parti» mostra strofe, ritornelli, stacchi… riconosciuti dall'analisi. Clicca una parte per modificarla, trascina il bordo per spostarla."));
                ui.label(ts("• Ogni «Scena» definisce colori, intensità, effetti e movimenti di ogni gruppo di fari fino alla scena successiva. Ogni scena diventa una nota MIDI."));
                ui.label(ts("• I «Colpi» sono accenti sopra la scena: accecatori, strobo, flash, blackout negli stacchi."));
                ui.label(ts("• Premi ▶ per l'anteprima con l'audio delle tracce."));
                ui.separator();
                if let Some(s) = self.cur_song() {
                    if let Some(a) = &s.analysis {
                        ui.label(RichText::new(&s.name).strong());
                        ui.label(crate::i18n::t(&format!("{:.1} BPM · {}/4 · {}", a.bpm, a.beats_per_bar, a.character)));
                        ui.label(crate::i18n::t(&format!("{} parti, {} scene, {} colpi", s.sections.len(), s.cues.len(), s.hits.len())));
                    }
                }
            }
        }
    }

    fn inspect_section(&mut self, ui: &mut egui::Ui, i: usize) {
        let ana = self.cur_song().and_then(|s| s.analysis.clone()).unwrap_or_default();
        let mut regen = false;
        let mut jump = None;
        {
            let Some(s) = self.cur_song_mut() else { return };
            let Some(sec) = s.sections.get_mut(i) else { return };
            ui.heading(ts("Parte del brano"));
            ui.horizontal(|ui| {
                ui.label(ts("Nome"));
                ui.text_edit_singleline(&mut sec.label);
            });
            ui.horizontal(|ui| {
                ui.label(ts("Tipo"));
                egui::ComboBox::from_id_salt("sec_kind").selected_text(sec.kind.label()).show_ui(ui, |ui| {
                    for k in SectionKind::ALL {
                        ui.selectable_value(&mut sec.kind, k, k.label());
                    }
                });
            });
            ui.horizontal(|ui| {
                ui.label(ts("Intensità"));
                ui.add(egui::Slider::new(&mut sec.intensity, 0.0..=1.0).show_value(false));
                ui.label(intensity_label(sec.intensity));
            });
            ui.label(crate::i18n::t(&format!(
                "Da {} (battuta {}) a {} — {:.0} battute",
                fmt_time(sec.start),
                ana.bar_beat(sec.start),
                fmt_time(sec.end),
                ((sec.end - sec.start) / ana.bar_len().max(0.1)).round()
            )));
            ui.add_space(6.0);
            if ui.button(ts("▶ Ascolta da qui")).clicked() {
                jump = Some(sec.start);
            }
            if ui.button(ts("✨ Rigenera le luci di questa parte")).on_hover_text(ts("Ricrea scene e colpi di questa parte secondo tipo e intensità")).clicked() {
                regen = true;
            }
        }
        if let Some(t) = jump {
            self.seek(t);
            self.play();
        }
        if regen {
            self.push_undo();
            if let Some(s) = self.cur_song_mut() {
                let (a, b) = (s.sections[i].start, s.sections[i].end);
                let (cues, hits) = autoshow::generate(s);
                s.cues.retain(|c| c.time < a - 1e-6 || c.time >= b - 1e-6);
                s.cues.extend(cues.into_iter().filter(|c| c.time >= a - 1e-6 && c.time < b - 1e-6));
                s.hits.retain(|h| h.time < a - 1e-6 || h.time >= b - 1e-6);
                s.hits.extend(hits.into_iter().filter(|h| h.time >= a - 1e-6 && h.time < b - 1e-6));
                s.sort();
            }
            self.status = ts("Luci della parte rigenerate").into();
        }
    }

    fn inspect_hit(&mut self, ui: &mut egui::Ui, i: usize) {
        let mut delete = false;
        {
            let Some(s) = self.cur_song_mut() else { return };
            let Some(h) = s.hits.get_mut(i) else { return };
            ui.heading(ts("Colpo"));
            egui::ComboBox::from_id_salt("hit_kind_edit").selected_text(h.kind.label()).show_ui(ui, |ui| {
                for k in HitKind::ALL {
                    ui.selectable_value(&mut h.kind, k, k.label());
                }
            });
            ui.horizontal(|ui| {
                ui.label(ts("Istante (s)"));
                ui.add(egui::DragValue::new(&mut h.time).speed(0.01).range(0.0..=10000.0));
            });
            ui.horizontal(|ui| {
                ui.label(ts("Durata (s)"));
                ui.add(egui::Slider::new(&mut h.duration, 0.02..=4.0).logarithmic(true));
            });
            ui.label(match h.kind {
                HitKind::Blinder => ts("Gli accecatori sparano al massimo e poi si spengono rapidamente."),
                HitKind::Strobe => ts("Gli strobo (o i PAR, se non ci sono strobo) lampeggiano per la durata."),
                HitKind::Flash => ts("Tutti i fari fanno un flash bianco."),
                HitKind::Blackout => ts("Tutto spento per la durata: ideale per gli stacchi."),
            });
            if ui.button(ts("🗑 Elimina colpo")).clicked() {
                delete = true;
            }
        }
        if delete {
            self.delete_hit(i);
        }
    }

    fn inspect_cue(&mut self, ui: &mut egui::Ui, i: usize) {
        let groups: Vec<(Group, usize, bool)> = Group::ALL
            .iter()
            .map(|g| {
                let members: Vec<&FixtureInstance> = self.project.fixtures.iter().filter(|f| f.group == *g).collect();
                let moving = members.iter().any(|f| self.project.def(&self.catalog, &f.def_id).map(|d| d.kind.is_moving()).unwrap_or(false));
                (*g, members.len(), moving)
            })
            .collect();
        let ana = self.cur_song().and_then(|s| s.analysis.clone()).unwrap_or_default();
        let show_all = self.show_all_groups;
        let mut delete = false;
        let mut dup = false;
        let mut jump = None;
        let mut copy_prev = false;
        {
            let Some(s) = self.cur_song_mut() else { return };
            let ncues = s.cues.len();
            let Some(c) = s.cues.get_mut(i) else { return };
            ui.heading(crate::i18n::t(&format!("Scena {i}")));
            ui.horizontal(|ui| {
                ui.label(ts("Nome"));
                ui.text_edit_singleline(&mut c.name);
            });
            ui.horizontal(|ui| {
                ui.label(ts("Inizio (s)"));
                ui.add(egui::DragValue::new(&mut c.time).speed(0.01).range(0.0..=10000.0));
                ui.label(crate::i18n::t(&format!("battuta {}", ana.bar_beat(c.time))));
            });
            ui.horizontal(|ui| {
                ui.label(ts("Dissolvenza (s)"));
                ui.add(egui::Slider::new(&mut c.fade, 0.0..=8.0));
            });
            ui.label(
                RichText::new(if i <= crate::live::MAX_CUE_NOTE as usize {
                    crate::i18n::t(&format!("Nota MIDI: {} ({})", i, crate::midi::note_name(i as u8)))
                } else {
                    ts("Oltre la scena 119 non c'è nota MIDI dedicata").to_string()
                })
                .color(Color32::from_rgb(150, 200, 255)),
            );
            ui.horizontal(|ui| {
                if ui.button(ts("▶ Da qui")).clicked() {
                    jump = Some(c.time);
                }
                if ui.button(ts("⧉ Duplica")).clicked() {
                    dup = true;
                }
                if ui.add_enabled(i > 0, egui::Button::new(ts("⇐ Copia la precedente"))).clicked() {
                    copy_prev = true;
                }
                if ui.button("🗑").on_hover_text(ts("Elimina scena")).clicked() {
                    delete = true;
                }
            });
            let _ = ncues;
            ui.separator();
            for (g, count, moving) in &groups {
                if *count == 0 && !show_all {
                    continue;
                }
                let look = &mut c.looks[g.idx()];
                let col = look.colors.first().copied().unwrap_or([255, 255, 255]);
                let title = RichText::new(crate::i18n::t(&format!("{}  ({} fari) — {}%", g.label(), count, (look.intensity * 100.0) as i32))).color(if look.intensity > 0.01 {
                    c32(col)
                } else {
                    Color32::GRAY
                });
                egui::CollapsingHeader::new(title).id_salt(("grp", g.idx())).default_open(look.intensity > 0.0 && *count > 0).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(ts("Intensità"));
                        ui.add(egui::Slider::new(&mut look.intensity, 0.0..=1.0));
                    });
                    ui.horizontal_wrapped(|ui| {
                        ui.label(ts("Colori"));
                        let mut remove = None;
                        let n = look.colors.len();
                        for (k, c) in look.colors.iter_mut().enumerate() {
                            ui.color_edit_button_srgb(c);
                            if n > 1 && ui.small_button("×").clicked() {
                                remove = Some(k);
                            }
                        }
                        if let Some(k) = remove {
                            look.colors.remove(k);
                        }
                        if look.colors.len() < 6 && ui.small_button("+").clicked() {
                            let last = *look.colors.last().unwrap_or(&[255, 255, 255]);
                            look.colors.push(last);
                        }
                    });
                    ui.horizontal(|ui| {
                        for sw in SWATCHES {
                            let (r, resp) = ui.allocate_exact_size(vec2(16.0, 16.0), Sense::click());
                            ui.painter().rect_filled(r, 3.0, c32(sw));
                            if resp.on_hover_text(ts("Clic: usa questo colore · Maiusc+clic: aggiungi")).clicked() {
                                if ui.input(|i| i.modifiers.shift) {
                                    look.colors.push(sw);
                                } else {
                                    look.colors = vec![sw];
                                }
                            }
                        }
                    });
                    ui.horizontal(|ui| {
                        ui.label(ts("Effetto"));
                        egui::ComboBox::from_id_salt(("eff", g.idx())).selected_text(look.effect.label()).width(150.0).show_ui(ui, |ui| {
                            for e in Effect::ALL {
                                ui.selectable_value(&mut look.effect, e, e.label());
                            }
                        });
                    });
                    if !matches!(look.effect, Effect::Static | Effect::Off | Effect::Kick | Effect::Snare | Effect::Sparkle) {
                        ui.horizontal(|ui| {
                            ui.label(ts("Velocità"));
                            rate_combo(ui, ("rate", g.idx()), &mut look.rate, &RATES);
                        });
                    }
                    if *moving || show_all {
                        ui.horizontal(|ui| {
                            ui.label(ts("Movimento"));
                            egui::ComboBox::from_id_salt(("mv", g.idx())).selected_text(look.movement.label()).width(150.0).show_ui(ui, |ui| {
                                for m in Movement::ALL {
                                    ui.selectable_value(&mut look.movement, m, m.label());
                                }
                            });
                        });
                        if look.movement != Movement::Still {
                            ui.horizontal(|ui| {
                                ui.label(ts("Ampiezza"));
                                ui.add(egui::Slider::new(&mut look.move_size, 0.0..=1.0));
                            });
                            ui.horizontal(|ui| {
                                ui.label(ts("Ciclo"));
                                rate_combo(ui, ("mrate", g.idx()), &mut look.move_rate, &MOVE_RATES);
                            });
                            ui.checkbox(&mut look.mirror, ts("⇋ Specchia sinistra / destra"))
                                .on_hover_text(ts("Le teste a sinistra del palco fanno lo stesso movimento ma a specchio (es. cerchio verso sinistra mentre a destra va verso destra)"));
                        }
                        ui.horizontal(|ui| {
                            ui.label(ts("Inclinazione"));
                            ui.add(egui::Slider::new(&mut look.tilt, -40.0..=85.0).suffix("°"));
                        });
                    }
                    ui.horizontal(|ui| {
                        ui.label(ts("Sfasamento"));
                        ui.add(egui::Slider::new(&mut look.spread, 0.0..=1.0));
                    });
                    ui.horizontal(|ui| {
                        ui.label(ts("Strobo"));
                        ui.add(egui::Slider::new(&mut look.strobe, 0.0..=1.0));
                    });
                });
            }
        }
        ui.checkbox(&mut self.show_all_groups, ts("Mostra anche i gruppi senza fari"));
        if let Some(t) = jump {
            self.seek(t);
            self.play();
        }
        if copy_prev {
            self.push_undo();
            if let Some(s) = self.cur_song_mut() {
                if i > 0 && i < s.cues.len() {
                    s.cues[i].looks = s.cues[i - 1].looks.clone();
                }
            }
        }
        if dup {
            self.push_undo();
            let bar = ana.bar_len().max(0.5);
            if let Some(s) = self.cur_song_mut() {
                let mut c = s.cues[i].clone();
                c.time += bar;
                c.name = crate::i18n::t(&format!("{} (copia)", c.name));
                s.cues.insert(i + 1, c);
                s.cues.sort_by(|a, b| a.time.total_cmp(&b.time));
            }
            self.sel = Sel::Cue(i + 1);
        }
        if delete {
            self.delete_cue(i);
        }
    }
}