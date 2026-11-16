//! "Live" tab: MIDI input, DMX output, live status and universe monitor.

use crate::app::*;
use crate::live::*;
use crate::midi::MidiIn;
use crate::model::*;
use crate::stage;
use egui::{pos2, vec2, Color32, Rect, RichText, Sense};

impl App {
    pub fn ui_live(&mut self, ctx: &egui::Context) {
        egui::SidePanel::left("midi").default_width(320.0).resizable(true).show(ctx, |ui| {
            ui.heading("Ingresso MIDI");
            ui.label(RichText::new("Collega qui l'uscita MIDI del player delle basi (interfaccia USB-MIDI o cavo virtuale).").size(11.0).color(Color32::GRAY));
            ui.horizontal(|ui| {
                egui::ComboBox::from_id_salt("midi_port")
                    .width(200.0)
                    .selected_text(if self.midi_choice.is_empty() { "nessuna porta" } else { &self.midi_choice })
                    .show_ui(ui, |ui| {
                        for p in &self.midi_ports {
                            ui.selectable_value(&mut self.midi_choice, p.clone(), p);
                        }
                    });
                if ui.button("⟳").on_hover_text("Aggiorna l'elenco").clicked() {
                    self.midi_ports = MidiIn::ports();
                    if !self.midi_ports.contains(&self.midi_choice) {
                        self.midi_choice = self.midi_ports.first().cloned().unwrap_or_default();
                    }
                }
            });
            ui.horizontal(|ui| {
                if self.midi.conn.is_none() {
                    if ui.add_enabled(!self.midi_choice.is_empty(), egui::Button::new(RichText::new("▶ Ascolta").strong())).clicked() {
                        let name = self.midi_choice.clone();
                        self.midi.connect(&name, self.shared.clone());
                    }
                } else {
                    ui.label(RichText::new(format!("● in ascolto su {}", self.midi.port_name)).color(Color32::from_rgb(80, 220, 120)));
                    if ui.button("Stop").clicked() {
                        self.midi.disconnect();
                    }
                }
            });
            if !self.midi.error.is_empty() {
                ui.label(RichText::new(&self.midi.error).color(Color32::from_rgb(255, 140, 80)));
            }
            ui.horizontal(|ui| {
                ui.label("Canale MIDI");
                let ch = &mut self.project.settings.midi_channel;
                egui::ComboBox::from_id_salt("midi_ch").selected_text(if *ch == 0 { "tutti".to_string() } else { ch.to_string() }).show_ui(ui, |ui| {
                    ui.selectable_value(ch, 0, "tutti");
                    for c in 1..=16u8 {
                        ui.selectable_value(ch, c, c.to_string());
                    }
                });
            });
            ui.checkbox(&mut self.project.settings.stop_note_at_end, "Nella traccia MIDI: nota di stop a fine brano");
            ui.separator();
            ui.label(RichText::new("Cosa capisce l'app").strong());
            for l in [
                "Program Change N → seleziona il brano N".to_string(),
                format!("Note 0-{MAX_CUE_NOTE} → vai alla scena con quel numero e parti"),
                format!("Nota {NOTE_PLAY_FROM_START} → show dall'inizio"),
                format!("Nota {NOTE_STOP} → stop · Nota {NOTE_BLACKOUT} → blackout on/off"),
                "MIDI Start / Continue / Stop / Song Position".into(),
                "CC 7 → intensità generale".into(),
            ] {
                ui.label(RichText::new(format!("• {l}")).size(12.0));
            }
            ui.horizontal(|ui| {
                if ui.button("💾 Esporta MIDI del brano").clicked() {
                    self.export_midi(false);
                }
                if ui.button("…di tutti").clicked() {
                    self.export_midi(true);
                }
            });
            ui.separator();
            ui.horizontal(|ui| {
                ui.label(RichText::new("Prova senza player:").strong());
            });
            ui.horizontal(|ui| {
                let prog = self.cur_song().map(|s| s.program).unwrap_or(0);
                let ch = self.project.settings.midi_channel.clamp(1, 16) - 1;
                if ui.button(format!("PC {prog}")).clicked() {
                    self.push_project();
                    if let Ok(mut s) = self.shared.lock() {
                        s.handle_midi(&[0xC0 | ch, prog], std::time::Instant::now());
                    }
                }
                if ui.button("Nota 0 (parti)").clicked() {
                    self.push_project();
                    if let Ok(mut s) = self.shared.lock() {
                        s.handle_midi(&[0x90 | ch, 0, 100], std::time::Instant::now());
                    }
                }
                if ui.button("Nota stop").clicked() {
                    if let Ok(mut s) = self.shared.lock() {
                        s.handle_midi(&[0x90 | ch, NOTE_STOP, 100], std::time::Instant::now());
                    }
                }
            });
            ui.separator();
            ui.label(RichText::new("Messaggi ricevuti").strong());
            let log: Vec<String> = self.shared.lock().map(|s| s.midi_log.iter().cloned().collect()).unwrap_or_default();
            egui::ScrollArea::vertical().id_salt("midilog").show(ui, |ui| {
                if log.is_empty() {
                    ui.label(RichText::new("Nessun messaggio ancora").color(Color32::GRAY));
                }
                for l in log {
                    ui.label(RichText::new(l).monospace().size(10.5));
                }
            });
        });

        egui::SidePanel::right("dmx").default_width(340.0).resizable(true).show(ctx, |ui| {
            ui.heading("Uscita DMX");
            ui.horizontal(|ui| {
                ui.label("Tipo interfaccia");
                let p = &mut self.project.settings.dmx_protocol;
                egui::ComboBox::from_id_salt("proto").width(200.0).selected_text(p.label()).show_ui(ui, |ui| {
                    ui.selectable_value(p, DmxProtocol::OpenDmx, DmxProtocol::OpenDmx.label());
                    ui.selectable_value(p, DmxProtocol::EnttecPro, DmxProtocol::EnttecPro.label());
                });
            });
            ui.horizontal(|ui| {
                ui.label("Porta");
                egui::ComboBox::from_id_salt("dmx_port")
                    .width(200.0)
                    .selected_text(if self.dmx_choice.is_empty() { "nessuna" } else { &self.dmx_choice })
                    .show_ui(ui, |ui| {
                        for p in &self.dmx_ports {
                            ui.selectable_value(&mut self.dmx_choice, p.name.clone(), format!("{} {}", p.name, p.desc));
                        }
                    });
                if ui.button("⟳").clicked() {
                    self.dmx_ports = crate::dmx::ports();
                    if !self.dmx_ports.iter().any(|p| p.name == self.dmx_choice) {
                        self.dmx_choice = self.dmx_ports.first().map(|p| p.name.clone()).unwrap_or_default();
                    }
                }
            });
            ui.horizontal(|ui| {
                ui.label("Frequenza");
                ui.add(egui::Slider::new(&mut self.project.settings.dmx_fps, 10..=44).suffix(" Hz"));
            });
            let (connected, status, frames) =
                self.shared.lock().map(|s| (s.dmx.port.is_some(), s.dmx_status.clone(), s.dmx_frames)).unwrap_or((false, String::new(), 0));
            ui.horizontal(|ui| {
                if !connected {
                    if ui.add_enabled(!self.dmx_choice.is_empty(), egui::Button::new(RichText::new("▶ Collega").strong())).clicked() {
                        self.push_project();
                        if let Ok(mut s) = self.shared.lock() {
                            s.dmx.port = Some(self.dmx_choice.clone());
                        }
                    }
                } else if ui.button("Scollega").clicked() {
                    if let Ok(mut s) = self.shared.lock() {
                        s.dmx.port = None;
                    }
                }
            });
            ui.label(RichText::new(&status).color(if status.starts_with("Collegato") { Color32::from_rgb(80, 220, 120) } else { Color32::GRAY }));
            if connected {
                ui.label(RichText::new(format!("{frames} pacchetti inviati")).size(11.0).color(Color32::GRAY));
            }
            if self.dmx_ports.is_empty() {
                ui.label(RichText::new("Nessuna porta seriale trovata: collega l'interfaccia USB-DMX e premi ⟳. Su Windows può servire il driver FTDI (VCP).").size(11.0).color(Color32::from_rgb(255, 170, 90)));
            }
            ui.separator();
            ui.horizontal(|ui| {
                let mut bo = self.shared.lock().map(|s| s.blackout).unwrap_or(false);
                if ui.toggle_value(&mut bo, RichText::new("⬛ BLACKOUT").strong()).changed() {
                    if let Ok(mut s) = self.shared.lock() {
                        s.blackout = bo;
                    }
                }
                if ui.toggle_value(&mut self.test_all, "Test: tutti i canali al 100%").changed() {
                    if let Ok(mut s) = self.shared.lock() {
                        s.test_channels = if self.test_all { Some((0..512).map(|c| (c, 255u8)).collect()) } else { None };
                    }
                }
            });
            ui.horizontal(|ui| {
                ui.label("Intensità generale");
                ui.add(egui::Slider::new(&mut self.project.settings.master, 0.0..=1.0));
            });
            ui.separator();
            ui.label(RichText::new("Monitor universo DMX (512 canali)").strong());
            let uni = self.shared.lock().map(|s| s.universe).unwrap_or([0; 512]);
            let cols = 16usize;
            let w = ui.available_width();
            let cell = (w / cols as f32).floor();
            let (r, painter) = ui.allocate_painter(vec2(w, cell * 0.7 * 32.0), Sense::hover());
            let ch_h = cell * 0.7;
            for (i, v) in uni.iter().enumerate() {
                let x = r.rect.left() + (i % cols) as f32 * cell;
                let y = r.rect.top() + (i / cols) as f32 * ch_h;
                let rc = Rect::from_min_size(pos2(x, y), vec2(cell - 1.0, ch_h - 1.0));
                let k = *v as f32 / 255.0;
                painter.rect_filled(rc, 1.0, Color32::from_rgb((25.0 + 200.0 * k) as u8, (25.0 + 150.0 * k) as u8, (35.0 + 30.0 * k) as u8));
                if let Some(h) = r.hover_pos() {
                    if rc.contains(h) {
                        let owner = self
                            .project
                            .fixtures
                            .iter()
                            .find(|f| {
                                let n = self.project.def(&self.catalog, &f.def_id).map(|d| d.footprint()).unwrap_or(1) as usize;
                                i + 1 >= f.address as usize && i + 1 < f.address as usize + n
                            })
                            .map(|f| f.name.clone())
                            .unwrap_or_else(|| "libero".into());
                        egui::show_tooltip_at_pointer(ui.ctx(), ui.layer_id(), egui::Id::new("dmx_tip"), |ui| {
                            ui.label(format!("Canale {} = {}  ({owner})", i + 1, v));
                        });
                    }
                }
            }
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            let (song_idx, playing, pos, by_midi, last_midi) = self
                .shared
                .lock()
                .map(|s| (s.transport.song, s.transport.playing, s.transport.pos(), s.transport.by_midi, s.last_midi))
                .unwrap_or((0, false, 0.0, false, None));
            let song = self.project.songs.get(song_idx);
            ui.horizontal(|ui| {
                let (c, t) = if playing { (Color32::from_rgb(255, 70, 70), "● IN ONDA") } else { (Color32::GRAY, "■ FERMO") };
                ui.label(RichText::new(t).size(22.0).strong().color(c));
                ui.label(RichText::new(song.map(|s| s.name.clone()).unwrap_or_else(|| "—".into())).size(22.0).strong());
                ui.label(RichText::new(fmt_time(pos)).monospace().size(22.0).color(Color32::from_rgb(255, 210, 90)));
            });
            if let Some(s) = song {
                let ci = s.cues.partition_point(|c| c.time <= pos);
                let cue = if ci > 0 { format!("Scena {}: {}", ci - 1, s.cues[ci - 1].name) } else { "prima della prima scena".into() };
                let sec = s.section_at(pos).map(|i| s.sections[i].label.clone()).unwrap_or_default();
                ui.label(RichText::new(format!("{cue}   ·   {sec}")).size(15.0));
            }
            ui.label(
                RichText::new(match (by_midi, last_midi) {
                    (true, _) => "Comandato dal MIDI esterno".to_string(),
                    (false, Some(t)) => format!("Ultimo messaggio MIDI {:.0} s fa", t.elapsed().as_secs_f32()),
                    _ => "In attesa di MIDI: il player delle basi deve inviare Program Change e note della traccia esportata".to_string(),
                })
                .color(Color32::GRAY),
            );
            ui.horizontal(|ui| {
                if ui.button("▶ Prova in locale").clicked() {
                    self.select_song(song_idx);
                    self.play();
                }
                if ui.button("⏹ Stop").clicked() {
                    self.stop();
                }
            });
            let avail = ui.available_size();
            let t = self.now();
            stage::draw_perspective(ui, vec2(avail.x, avail.y - 6.0), &self.project, &self.catalog, &self.states, None, t);
        });
    }
}
