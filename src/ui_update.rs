//! Update check UI: automatic check at start-up (at most twice a day),
//! banner when a new version exists, download + install.

use crate::app::*;
#[allow(unused_imports)]
use crate::i18n::{t, ts};
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
    /// When the last check was started (this session), to retry after a failure.
    pub last_auto_try: Option<std::time::Instant>,
    /// Whether the last check failed (then retry sooner).
    pub last_failed: bool,
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
            self.update.message = ts("Controllo in corso…").into();
        }
        self.update.last_auto_try = Some(std::time::Instant::now());
    }

    /// Seconds since the last check that succeeded (saved across sessions).
    fn since_last_success(&self) -> u64 {
        now_secs().saturating_sub(self.prefs.last_update_check)
    }

    /// Automatic check at start-up (if enabled), unless one succeeded in the last 2 hours.
    pub fn auto_update_check(&mut self) {
        if self.screenshot.is_none() && self.prefs.check_updates && self.since_last_success() >= 2 * 3600 {
            self.start_update_check(false);
        }
    }

    /// While the app stays open: check again 12 hours after the last successful check,
    /// or 10 minutes after a failed one.
    fn periodic_update_check(&mut self) {
        if self.screenshot.is_some() || !self.prefs.check_updates || self.update.rx.is_some() || self.update.dl_rx.is_some() || self.update.available.is_some() {
            return;
        }
        let due = if self.update.last_failed {
            self.update.last_auto_try.map_or(true, |t| t.elapsed().as_secs() >= 10 * 60)
        } else {
            self.since_last_success() >= 12 * 3600
        };
        if due {
            self.start_update_check(false);
        }
    }

    pub fn poll_update(&mut self, ctx: &egui::Context) {
        self.periodic_update_check();
        if let Some(rx) = &self.update.rx {
            if let Ok(r) = rx.try_recv() {
                self.update.rx = None;
                self.update.last_failed = r.is_err();
                if r.is_ok() {
                    self.prefs.last_update_check = now_secs();
                }
                match r {
                    Ok(Some(info)) => {
                        let skipped = !self.update.manual && self.prefs.skip_version == info.version;
                        self.update.message = crate::i18n::t(&format!("È disponibile la versione {}", info.version));
                        if !skipped {
                            self.update.dismissed = false;
                            self.update.available = Some(info);
                        }
                    }
                    Ok(None) => self.update.message = crate::i18n::t(&format!("Hai già l'ultima versione ({}).", update::current_version())),
                    Err(e) => {
                        self.update.message = crate::i18n::t(&format!("Controllo aggiornamenti non riuscito: {e}"));
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
                            self.status = ts("Installo l'aggiornamento: LightSciò si riapre da solo al termine.").into();
                            // save the project first, then close so the installer can replace the program
                            if self.path.is_some() {
                                self.save(false);
                            }
                            self.force_close = true;
                            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                        } else if cfg!(target_os = "macos") {
                            self.update.message = ts("Si è aperta l'immagine disco: trascina LightSciò nella cartella Applicazioni, sostituendo la versione vecchia, poi riapri l'app.").into();
                            self.status = self.update.message.clone();
                        } else {
                            self.update.message = crate::i18n::t(&format!("Scaricato in {}", p.display()));
                        }
                    }
                    Err(e) => {
                        self.update.message = crate::i18n::t(&format!("Aggiornamento non riuscito: {e}"));
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
            let url = if info.page.is_empty() { crate::i18n::t(&format!("https://github.com/{}/releases/latest", update::normalize_repo(&self.prefs.update_repo))) } else { info.page.clone() };
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
                ui.label(RichText::new(crate::i18n::t(&format!("⬆ È disponibile LightSciò {} (tu hai la {})", info.version, update::current_version()))).strong().color(Color32::from_rgb(255, 210, 110)));
                if let Some(p) = &self.update.dl_progress {
                    let v = p.lock().map(|x| *x).unwrap_or(0.0);
                    ui.add(egui::ProgressBar::new(v).desired_width(220.0).text(crate::i18n::t(&format!("Scarico… {:.0}%", v * 100.0))));
                } else {
                    let label = if info.asset_url.is_some() {
                        if cfg!(target_os = "windows") {
                            ts("Scarica e installa")
                        } else {
                            ts("Scarica")
                        }
                    } else {
                        ts("Apri la pagina di download")
                    };
                    if ui.button(RichText::new(label).strong()).clicked() {
                        self.start_download();
                    }
                    if ui.button(ts("Novità")).clicked() {
                        self.update.show_notes = !self.update.show_notes;
                    }
                    if ui.button(ts("Più tardi")).clicked() {
                        self.update.dismissed = true;
                    }
                    if ui.button(ts("Salta questa versione")).clicked() {
                        self.prefs.skip_version = info.version.clone();
                        self.update.dismissed = true;
                    }
                }
            });
            if self.update.show_notes {
                egui::ScrollArea::vertical().max_height(160.0).show(ui, |ui| {
                    ui.label(if info.notes.trim().is_empty() { ts("(nessuna nota di rilascio)") } else { info.notes.as_str() });
                    if ui.link(ts("Pagina della versione")).clicked() {
                        self.open_release_page();
                    }
                });
            }
        });
    }

    /// Update section of the Preferences window.
    pub fn ui_update_prefs(&mut self, ui: &mut egui::Ui) {
        ui.separator();
        ui.label(RichText::new(ts("Aggiornamenti")).strong());
        ui.label(crate::i18n::t(&format!("Versione installata: {}", update::current_version())));
        ui.checkbox(&mut self.prefs.check_updates, ts("Controlla automaticamente all'avvio"));
        ui.horizontal(|ui| {
            ui.label(ts("Progetto GitHub"));
            ui.add(egui::TextEdit::singleline(&mut self.prefs.update_repo).desired_width(220.0).hint_text(update::DEFAULT_REPO));
        });
        ui.horizontal(|ui| {
            let busy = self.update.rx.is_some();
            if ui.add_enabled(!busy, egui::Button::new(ts("Controlla ora"))).clicked() {
                self.update.dismissed = false;
                self.start_update_check(true);
            }
            if busy {
                ui.spinner();
            }
        });
        if !self.update.message.is_empty() {
            ui.label(RichText::new(t(&self.update.message)).size(11.5).color(Color32::from_rgb(190, 190, 205)));
        }
        if self.update.available.is_some() && self.update.dismissed && ui.button(ts("Mostra l'aggiornamento disponibile")).clicked() {
            self.update.dismissed = false;
        }
    }
}
