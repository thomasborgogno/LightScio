//! Fixture data sheet: physical data, links, DMX channels with what every
//! range does, wheels and slots (data from the fixture library).

use crate::app::App;
use crate::i18n::{t, ts};
use crate::model::*;
use egui::{vec2, Color32, RichText};

/// Italian name of an Open Fixture Library capability type.
fn type_label(typ: &str) -> &'static str {
    match typ {
        "NoFunction" => ts("Nessuna funzione"),
        "ShutterStrobe" => ts("Shutter"),
        "StrobeSpeed" => ts("Velocità strobo"),
        "StrobeDuration" => ts("Durata strobo"),
        "Intensity" => ts("Intensità"),
        "ColorIntensity" => ts("Intensità colore"),
        "ColorPreset" => ts("Colore preimpostato"),
        "ColorTemperature" => ts("Temperatura colore"),
        "Pan" => "Pan",
        "PanContinuous" => ts("Pan continuo"),
        "Tilt" => "Tilt",
        "TiltContinuous" => ts("Tilt continuo"),
        "PanTiltSpeed" => ts("Velocità pan/tilt"),
        "WheelSlot" => ts("Ruota"),
        "WheelShake" => ts("Scuotimento ruota"),
        "WheelSlotRotation" => ts("Rotazione slot"),
        "WheelRotation" => ts("Rotazione ruota"),
        "Effect" => ts("Effetto"),
        "EffectSpeed" => ts("Velocità effetto"),
        "EffectDuration" => ts("Durata effetto"),
        "EffectParameter" => ts("Parametro effetto"),
        "SoundSensitivity" => ts("Sensibilità al suono"),
        "BeamAngle" => ts("Angolo fascio"),
        "BeamPosition" => ts("Posizione fascio"),
        "Focus" => ts("Fuoco"),
        "Zoom" => "Zoom",
        "Iris" => ts("Iride"),
        "IrisEffect" => ts("Effetto iride"),
        "Frost" => "Frost",
        "FrostEffect" => ts("Effetto frost"),
        "Prism" => ts("Prisma"),
        "PrismRotation" => ts("Rotazione prisma"),
        "BladeInsertion" => ts("Lama"),
        "BladeRotation" => ts("Rotazione lama"),
        "BladeSystemRotation" => ts("Rotazione sistema lame"),
        "Fog" => ts("Fumo"),
        "FogOutput" => ts("Uscita fumo"),
        "FogType" => ts("Tipo di fumo"),
        "Rotation" => ts("Rotazione"),
        "Speed" => ts("Velocità"),
        "Time" => ts("Tempo"),
        "Maintenance" => ts("Manutenzione"),
        "Generic" => ts("Generico"),
        _ => "",
    }
}

/// Italian name of a shutter effect / wheel slot type.
fn effect_label(e: &str) -> Option<&'static str> {
    Some(match e {
        "Open" => ts("aperto"),
        "Closed" => ts("chiuso"),
        "Strobe" => ts("strobo"),
        "Pulse" => ts("impulso"),
        "RampUp" => ts("rampa su"),
        "RampDown" => ts("rampa giù"),
        "RampUpDown" => ts("rampa su/giù"),
        "Lightning" => ts("lampi"),
        "Spikes" => ts("picchi"),
        "Burst" => ts("raffica"),
        "Color" => ts("colore"),
        "Gobo" => "gobo",
        "Prism" => ts("prisma"),
        "Iris" => ts("iride"),
        "Frost" => "frost",
        "AnimationGoboStart" => ts("gobo animato (inizio)"),
        "AnimationGoboEnd" => ts("gobo animato (fine)"),
        _ => return None,
    })
}

/// One line describing what a DMX range does.
pub fn cap_text(c: &CapInfo) -> String {
    let mut out = match c.typ.as_str() {
        "Fine" => t(&format!("Regolazione fine di {}", c.what)),
        // the wheel name is already in `what`
        "WheelSlot" => c.what.clone(),
        "ShutterStrobe" => {
            let mut w = c.what.splitn(2, ' ');
            let first = w.next().unwrap_or("");
            let rest = w.next().map(|r| format!(" {r}")).unwrap_or_default();
            format!("{} {}{rest}", ts("Shutter"), effect_label(first).unwrap_or(first))
        }
        typ => {
            let label = type_label(typ);
            let label = if label.is_empty() { typ } else { label };
            if c.what.is_empty() {
                label.to_string()
            } else {
                format!("{label}: {}", c.what)
            }
        }
    };
    if !c.values.is_empty() {
        out.push_str(&format!(" · {}", c.values));
    }
    if !c.comment.is_empty() {
        out.push_str(&format!(" — {}", c.comment));
    }
    out
}

fn swatches(ui: &mut egui::Ui, colors: &[[u8; 3]]) {
    for c in colors.iter().take(4) {
        let (r, _) = ui.allocate_exact_size(vec2(11.0, 11.0), egui::Sense::hover());
        ui.painter().rect_filled(r, 2.0, Color32::from_rgb(c[0], c[1], c[2]));
        ui.painter().rect_stroke(r, 2.0, egui::Stroke::new(1.0, Color32::from_gray(90)), egui::StrokeKind::Inside);
    }
}

/// Table "DMX range — what it does" for one channel.
pub fn caps_grid(ui: &mut egui::Ui, id: impl std::hash::Hash, caps: &[CapInfo]) {
    egui::Grid::new(id).num_columns(2).spacing([10.0, 2.0]).striped(true).show(ui, |ui| {
        for c in caps {
            let range = if c.start == c.end { format!("{:>3}", c.start) } else { format!("{:>3}–{:<3}", c.start, c.end) };
            ui.label(RichText::new(range).monospace().size(10.5).color(Color32::GRAY));
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                swatches(ui, &c.colors);
                ui.add(egui::Label::new(RichText::new(cap_text(c)).size(11.0)).wrap());
            });
            ui.end_row();
        }
    });
}

/// Channel list: each channel opens to show its DMX ranges.
pub fn channels_ui(ui: &mut egui::Ui, salt: &str, d: &FixtureDef) {
    for (i, c) in d.channels.iter().enumerate() {
        let title = RichText::new(format!("{:>2}. {} — {}", i + 1, c.name, c.kind.label())).monospace().size(10.5);
        if c.caps.is_empty() {
            ui.label(title);
        } else {
            egui::CollapsingHeader::new(title).id_salt((salt, i)).show(ui, |ui| caps_grid(ui, (salt, "caps", i), &c.caps));
        }
    }
}

fn num(x: f32) -> String {
    if x.fract() == 0.0 {
        format!("{x:.0}")
    } else {
        format!("{x}")
    }
}

fn link_label(kind: &str) -> &'static str {
    match kind {
        "manual" => ts("Manuale"),
        "productPage" => ts("Pagina del prodotto"),
        "video" => "Video",
        _ => ts("Altro"),
    }
}

impl App {
    pub fn ui_fixture_info(&mut self, ctx: &egui::Context) {
        let Some(d) = self.info_def.clone() else { return };
        let mut open = true;
        egui::Window::new(ts("ℹ Scheda faro")).id(egui::Id::new("fixture_info")).open(&mut open).default_width(520.0).default_height(560.0).resizable(true).show(ctx, |ui| {
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                let info = d.info.as_deref();
                match info {
                    Some(i) => {
                        ui.heading(format!("{} {}", i.manufacturer, i.model));
                        let mode = if i.mode_short.is_empty() || i.mode_short == i.mode { i.mode.clone() } else { format!("{} ({})", i.mode, i.mode_short) };
                        ui.label(t(&format!("Modo DMX: {} · {} canali", mode, d.channels.len())));
                        if !i.categories.is_empty() {
                            ui.label(RichText::new(i.categories.join(", ")).color(Color32::GRAY));
                        }
                    }
                    None => {
                        ui.heading(&d.name);
                        ui.label(t(&format!("{} canali", d.channels.len())));
                    }
                }
                ui.label(RichText::new(format!("{} · {}", d.kind.label(), d.capabilities())).size(11.0));
                if let Some(i) = info {
                    if !i.comment.is_empty() {
                        ui.label(RichText::new(&i.comment).italics());
                    }
                    let mut rows: Vec<(&str, String)> = vec![];
                    if let Some([w, h, dp]) = i.dimensions {
                        rows.push((ts("Dimensioni (L × A × P)"), format!("{} × {} × {} mm", num(w), num(h), num(dp))));
                    }
                    if let Some(w) = i.weight {
                        rows.push((ts("Peso"), format!("{} kg", num(w))));
                    }
                    if let Some(p) = i.power {
                        rows.push((ts("Potenza"), format!("{} W", num(p))));
                    }
                    if !i.dmx_connector.is_empty() {
                        rows.push((ts("Connettore DMX"), i.dmx_connector.clone()));
                    }
                    if !i.bulb.is_empty() {
                        rows.push((ts("Sorgente luminosa"), i.bulb.clone()));
                    }
                    if let Some(k) = i.color_temperature {
                        rows.push((ts("Temperatura colore"), format!("{} K", num(k))));
                    }
                    if let Some(l) = i.lumens {
                        rows.push((ts("Flusso luminoso"), format!("{} lm", num(l))));
                    }
                    if !i.lens.is_empty() || i.lens_degrees.is_some() {
                        let deg = i.lens_degrees.map(|[a, b]| if a == b { format!("{}°", num(a)) } else { format!("{}°–{}°", num(a), num(b)) }).unwrap_or_default();
                        rows.push((ts("Ottica"), [i.lens.as_str(), deg.as_str()].iter().filter(|x| !x.is_empty()).cloned().collect::<Vec<_>>().join(" · ")));
                    }
                    if !i.focus_type.is_empty() {
                        rows.push((ts("Movimento"), i.focus_type.clone()));
                    }
                    if !i.pan_max.is_empty() {
                        rows.push((ts("Escursione pan"), i.pan_max.clone()));
                    }
                    if !i.tilt_max.is_empty() {
                        rows.push((ts("Escursione tilt"), i.tilt_max.clone()));
                    }
                    if let Some([x, y, z]) = i.matrix {
                        rows.push((ts("Matrice di pixel"), if z > 1 { format!("{x} × {y} × {z}") } else { format!("{x} × {y}") }));
                    }
                    if !rows.is_empty() {
                        ui.add_space(6.0);
                        ui.label(RichText::new(ts("Dati fisici")).strong());
                        egui::Grid::new("fi_phys").num_columns(2).spacing([14.0, 3.0]).striped(true).show(ui, |ui| {
                            for (k, v) in rows {
                                ui.label(RichText::new(k).color(Color32::GRAY));
                                ui.label(v);
                                ui.end_row();
                            }
                        });
                    }
                }
                ui.add_space(6.0);
                ui.label(RichText::new(ts("Canali DMX")).strong());
                ui.label(RichText::new(ts("Apri un canale per vedere cosa fa ogni intervallo di valori.")).size(11.0).color(Color32::GRAY));
                channels_ui(ui, "fi_ch", &d);
                if !d.wheels.is_empty() {
                    ui.add_space(6.0);
                    ui.label(RichText::new(ts("Ruote")).strong());
                    for (wi, w) in d.wheels.iter().enumerate() {
                        egui::CollapsingHeader::new(t(&format!("{} · {} slot", w.name, w.slots.len()))).id_salt(("fi_wheel", wi)).default_open(true).show(ui, |ui| {
                            egui::Grid::new(("fi_slots", wi)).num_columns(3).spacing([10.0, 2.0]).show(ui, |ui| {
                                for (si, sl) in w.slots.iter().enumerate() {
                                    ui.label(RichText::new(format!("{:>2}", si + 1)).monospace().color(Color32::GRAY));
                                    ui.horizontal(|ui| {
                                        ui.spacing_mut().item_spacing.x = 4.0;
                                        swatches(ui, &sl.colors);
                                        let typ = effect_label(&sl.typ).unwrap_or(sl.typ.as_str());
                                        ui.label(if sl.name.is_empty() { typ.to_string() } else { format!("{} ({typ})", sl.name) });
                                    });
                                    ui.label(RichText::new(&sl.detail).size(10.5).color(Color32::GRAY));
                                    ui.end_row();
                                }
                            });
                        });
                    }
                }
                if let Some(i) = info {
                    if !i.links.is_empty() {
                        ui.add_space(6.0);
                        ui.label(RichText::new(ts("Link")).strong());
                        for (k, u) in &i.links {
                            ui.horizontal(|ui| {
                                ui.label(RichText::new(link_label(k)).color(Color32::GRAY));
                                ui.hyperlink_to(u.as_str(), u.as_str());
                            });
                        }
                    }
                    ui.add_space(6.0);
                    ui.separator();
                    ui.horizontal_wrapped(|ui| {
                        ui.label(RichText::new(ts("Fonte:")).size(11.0).color(Color32::GRAY));
                        ui.hyperlink_to(RichText::new(format!("{} · {}", i.library, i.key)).size(11.0), format!("{}/{}", crate::ofl::SITE, i.key));
                        if !i.version.is_empty() {
                            ui.label(RichText::new(t(&format!("versione {}", &i.version[..i.version.len().min(7)]))).size(11.0).color(Color32::GRAY));
                        }
                    });
                    let mut meta = vec![];
                    if !i.authors.is_empty() {
                        meta.push(t(&format!("Autori: {}", i.authors.join(", "))));
                    }
                    if !i.created.is_empty() {
                        meta.push(t(&format!("creata il {}", i.created)));
                    }
                    if !i.modified.is_empty() {
                        meta.push(t(&format!("modificata il {}", i.modified)));
                    }
                    if !meta.is_empty() {
                        ui.label(RichText::new(meta.join(" · ")).size(11.0).color(Color32::GRAY));
                    }
                }
            });
        });
        if !open {
            self.info_def = None;
        }
    }
}
