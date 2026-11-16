//! Update check UI: automatic check at start-up (at most twice a day),
//! banner when a new version exists, download + install.

use crate::app::*;
use crate::update::{self, ReleaseInfo};
use egui::{Color32, RichText};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};

#[derive(Default)]
pub struct UpdateState {
    pub rx: Option<Receiver<Result<Option<ReleaseInfo>, String>>>,
    pub manual: bool,
    pub available: Option<ReleaseInfo>,
    pub dismissed: bool,
    pub message: String,
    pub dl_progress: Option<Arc<Mutex<f32>>>,
    pub dl_rx: Option<Receiver<Result<std::path::PathBuf, String>>>,
    pub show_notes: bool,
}

fn now_secs() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

impl App {
    pub fn start_update_check(&mut self, manual: bool) {
        if self.update.rx.is_some() {
            return;
        }
        let repo = self.prefs.update_repo.clone();
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(update::check(&repo));
        });
        self.update.rx = Some(rx);
        self.update.manual = manual;
        if manual {
            self.update.message = "Controllo in corso…".into();
        }
        self.prefs.last_update_check = now_secs();
    }

    /// Automatic check once at start-up (if enabled, and not more than every 12 hours).
    pub fn auto_update_check(&mut self) {
        if self.screenshot.is_none() && self.prefs.check_updates && now_secs().saturating_sub(self.prefs.last_update_check) > 12 * 3600 {
            self.start_update_check(false);
        }
    }

    pub fn poll_update(&mut self, ctx: &egui::Context) {
        if let Some(rx) = &self.update.rx {
            if let Ok(r) = rx.try_recv() {
                self.update.rx = None;
                match r {
                    Ok(Some(info)) => {
                        let skipped = !self.update.manual && self.prefs.skip_version == info.version;
                        self.update.message = format!("È disponibile la versione {}", info.version);
                        if !skipped {
                            self.update.dismissed = false;
                            self.update.available = Some(info);
                        }
                    }
                    Ok(None) => self.update.message = format!("Hai già l'ultima versione ({}).", update::current_version()),
                    Err(e) => {
                        self.update.message = format!("Controllo aggiornamenti non riuscito: {e}");
                        if self.update.manual {
                            self.status = self.update.message.clone();
                        }
                    }
                }
            }
        }
        if let Some(rx) = &self.update.dl_rx {
            if let Ok(r) = rx.try_recv() {
                self.update.dl_rx = None;
                self.update.dl_progress = None;
                match r.and_then(|p| update::launch_installer(&p).map(|_| p)) {
                    Ok(p) => {
                        if cfg!(target_os = "windows") {
                            self.status = "Installo l'aggiornamento: LightSciò si riapre da solo al termine.".into();
                            // save the project first, then close so the installer can replace the program
                            if self.path.is_some() {
                                self.save(false);
                            }
                            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                        } else if cfg!(target_os = "macos") {
                            self.update.message = "Si è aperta l'immagine disco: trascina LightSciò nella cartella Applicazioni, sostituendo la versione vecchia, poi riapri l'app.".into();
                            self.status = self.update.message.clone();
                        } else {
                            self.update.message = format!("Scaricato in {}", p.display());
                        }
                    }
                    Err(e) => {
                        self.update.message = format!("Aggiornamento non riuscito: {e}");
                        self.status = self.update.message.clone();
                    }
                }
            }
        }
    }

    fn start_download(&mut self) {
        let Some(info) = self.update.available.clone() else { return };
        if info.asset_url.is_none() {
            // no installer for this system: open the release page
            self.open_release_page();
            return;
        }
        let progress = Arc::new(Mutex::new(0f32));
        let p2 = progress.clone();
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(update::download(&info, p2));
        });
        self.update.dl_progress = Some(progress);
        self.update.dl_rx = Some(rx);
    }

    fn open_release_page(&mut self) {
        if let Some(info) = &self.update.available {
            let url = if info.page.is_empty() { format!("https://github.com/{}/releases/latest", update::normalize_repo(&self.prefs.update_repo)) } else { info.page.clone() };
            self.open_url = Some(url);
        }
    }

    /// Banner under the top bar when a new version is available.
    pub fn ui_update_banner(&mut self, ctx: &egui::Context) {
        if let Some(url) = self.open_url.take() {
            ctx.open_url(egui::OpenUrl::new_tab(url));
        }
        let Some(info) = self.update.available.clone() else { return };
        if self.update.dismissed {
            return;
        }
        egui::TopBottomPanel::top("update_banner").frame(egui::Frame::new().fill(Color32::from_rgb(52, 40, 12)).inner_margin(egui::Margin::symmetric(10, 6))).show(ctx, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(format!("⬆ È disponibile LightSciò {} (tu hai la {})", info.version, update::current_version())).strong().color(Color32::from_rgb(255, 210, 110)));
                if let Some(p) = &self.update.dl_progress {
                    let v = p.lock().map(|x| *x).unwrap_or(0.0);
                    ui.add(egui::ProgressBar::new(v).desired_width(220.0).text(format!("Scarico… {:.0}%", v * 100.0)));
                } else {
                    let label = if info.asset_url.is_some() {
                        if cfg!(target_os = "windows") {
                            "Scarica e installa"
                        } else {
                            "Scarica"
                        }
                    } else {
                        "Apri la pagina di download"
                    };
                    if ui.button(RichText::new(label).strong()).clicked() {
                        self.start_download();
                    }
                    if ui.button("Novità").clicked() {
                        self.update.show_notes = !self.update.show_notes;
                    }
                    if ui.button("Più tardi").clicked() {
                        self.update.dismissed = true;
                    }
                    if ui.button("Salta questa versione").clicked() {
                        self.prefs.skip_version = info.version.clone();
                        self.update.dismissed = true;
                    }
                }
            });
            if self.update.show_notes {
                egui::ScrollArea::vertical().max_height(160.0).show(ui, |ui| {
                    ui.label(if info.notes.trim().is_empty() { "(nessuna nota di rilascio)" } else { info.notes.as_str() });
                    if ui.link("Pagina della versione").clicked() {
                        self.open_release_page();
                    }
                });
            }
        });
    }

    /// Update section of the Preferences window.
    pub fn ui_update_prefs(&mut self, ui: &mut egui::Ui) {
        ui.separator();
        ui.label(RichText::new("Aggiornamenti").strong());
        ui.label(format!("Versione installata: {}", update::current_version()));
        ui.checkbox(&mut self.prefs.check_updates, "Controlla automaticamente all'avvio");
        ui.horizontal(|ui| {
            ui.label("Progetto GitHub");
            ui.add(egui::TextEdit::singleline(&mut self.prefs.update_repo).desired_width(220.0).hint_text(update::DEFAULT_REPO));
        });
        ui.horizontal(|ui| {
            let busy = self.update.rx.is_some();
            if ui.add_enabled(!busy, egui::Button::new("Controlla ora")).clicked() {
                self.update.dismissed = false;
                self.start_update_check(true);
            }
            if busy {
                ui.spinner();
            }
        });
        if !self.update.message.is_empty() {
            ui.label(RichText::new(&self.update.message).size(11.5).color(Color32::from_rgb(190, 190, 205)));
        }
        if self.update.available.is_some() && self.update.dismissed && ui.button("Mostra l'aggiornamento disponibile").clicked() {
            self.update.dismissed = false;
        }
    }
}
