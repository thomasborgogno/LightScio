#![cfg_attr(all(not(debug_assertions), target_os = "windows"), windows_subsystem = "windows")]

mod analysis;
mod app;
mod audio;
mod autostart;
mod autoshow;
mod catalog;
mod demo;
mod dmx;
mod engine;
mod i18n;
mod live;
mod midi;
mod lanes;
mod model;
mod ofl;
mod patch;
mod relink;
mod stage;
mod ui_analysis;
mod ui_convert;
mod ui_faders;
mod ui_fixture_info;
mod ui_live;
mod ui_rig;
mod ui_show;
mod ui_songs;
mod ui_update;
mod update;

use std::path::PathBuf;

fn main() -> eframe::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--version") {
        println!("LightSciò {}", update::current_version());
        return Ok(());
    }
    if args.iter().any(|a| a == "--check-update") {
        println!("versione installata: {}", update::current_version());
        let repo = args.iter().position(|a| a == "--check-update").and_then(|i| args.get(i + 1)).cloned().unwrap_or_else(|| update::DEFAULT_REPO.to_string());
        match update::check(&repo) {
            Ok(Some(r)) => println!("nuova versione {} — installer: {} ({})", r.version, r.asset_name, r.asset_url.unwrap_or_default()),
            Ok(None) => println!("nessun aggiornamento"),
            Err(e) => println!("errore: {e}"),
        }
        return Ok(());
    }
    if let Some(i) = args.iter().position(|a| a == "--test-ofl") {
        // optional argument: a folder with the OFL fixtures (default: the downloaded library)
        let dir = args.get(i + 1).map(PathBuf::from).or_else(|| ofl::lib_dir().map(|d| ofl::fixtures_dir(&d)));
        let Some(dir) = dir.filter(|d| d.is_dir()) else {
            println!("libreria OFL non trovata (avvia l'app per scaricarla, o indica la cartella fixtures)");
            return Ok(());
        };
        let list = ofl::scan(&dir);
        let (mut ok, mut modes, mut err, mut missing) = (0, 0, 0, 0);
        let mut kinds = std::collections::BTreeMap::new();
        for e in &list {
            match ofl::load_report(&e.path) {
                Ok((v, miss)) => {
                    ok += 1;
                    modes += v.len();
                    *kinds.entry(format!("{:?}", v[0].kind)).or_insert(0) += 1;
                    if !miss.is_empty() {
                        missing += 1;
                        if missing <= 10 {
                            println!("MISSING {}: {:?}", e.path.display(), miss.iter().take(4).collect::<Vec<_>>());
                        }
                    }
                }
                Err(x) => {
                    err += 1;
                    if err < 5 {
                        println!("ERR {}: {x}", e.path.display());
                    }
                }
            }
        }
        println!("indexed {} parsed {ok} modes {modes} errors {err} with-missing-channels {missing} kinds {kinds:?}", list.len());
        for q in ["dotz par", "sharpy"] {
            if let Some(e) = list.iter().find(|e| e.key.contains(q)) {
                for d in ofl::load(&e.path).unwrap_or_default().iter().take(2) {
                    println!("{} [{:?}] open={} strobe={}-{} wheel={}", d.name, d.kind, d.shutter_open, d.strobe_min, d.strobe_max, d.color_wheel.len());
                    println!("   {:?}", d.channels.iter().map(|c| format!("{}:{:?}={}", c.name, c.kind, c.default)).collect::<Vec<_>>());
                }
            }
        }
        return Ok(());
    }
    if args.iter().any(|a| a == "--dump-demo") {
        let (p, _, _) = app::demo_project();
        for s in &p.songs {
            let (bytes, list) = midi::export_song(s, &p.settings);
            let _ = std::fs::create_dir_all("shots");
            let _ = std::fs::write("shots/demo.mid", &bytes);
            println!("midi: {} bytes, {} events, first: {:?}", bytes.len(), list.len(), list.iter().take(3).map(|e| format!("{:.2} {}", e.time, e.what)).collect::<Vec<_>>());
            if let Some(a) = &s.analysis {
                println!("bpm {:.2} offset {:.3} bar {:.3} dur {:.1} char {}", a.bpm, a.offset, a.bar_len(), a.duration, a.character);
                println!("kicks {} snares {} stops {:?} accents {:?}", a.kicks.len(), a.snares.len(), a.stops, a.accents);
                for x in &s.sections {
                    println!("{:>7.2} {:>7.2}  bars {:>5.1}-{:>5.1}  {:?} {} I={:.2}", x.start, x.end, a.beat_at(x.start) / 4.0, a.beat_at(x.end) / 4.0, x.kind, x.label, x.intensity);
                }
            }
        }
        return Ok(());
    }
    let screenshot = args.iter().position(|a| a == "--screenshot").and_then(|i| args.get(i + 1)).map(PathBuf::from);
    // "lightscio show.luci" (double click on a show file)
    let open_file = args.iter().skip(1).find(|a| a.to_lowercase().ends_with(".luci")).map(PathBuf::from);

    let mut viewport = egui::ViewportBuilder::default()
        .with_title("LightSciò — light show da multitraccia")
        .with_inner_size([1440.0, 900.0])
        .with_min_inner_size([1000.0, 640.0]);
    if let Some(icon) = app_icon() {
        viewport = viewport.with_icon(std::sync::Arc::new(icon));
    }
    if screenshot.is_some() {
        viewport = viewport.with_active(false);
        #[cfg(target_os = "windows")]
        {
            viewport = viewport.with_window_level(egui::WindowLevel::AlwaysOnBottom);
        }
    }
    #[allow(unused_mut)]
    let mut options = eframe::NativeOptions { viewport, ..Default::default() };
    #[cfg(target_os = "macos")]
    if screenshot.is_some() {
        options.event_loop_builder = Some(Box::new(|builder| {
            use winit::platform::macos::EventLoopBuilderExtMacOS;
            builder.with_activate_ignoring_other_apps(false);
        }));
    }
    if screenshot.is_some() {
        options.persist_window = false;
    }
    migrate_storage();
    eframe::run_native(APP_NAME, options, Box::new(move |cc| Ok(Box::new(app::App::new(cc, screenshot, open_file)))))
}


pub const APP_NAME: &str = "LightSciò";

/// Window / taskbar icon.
fn app_icon() -> Option<egui::IconData> {
    let img = image::load_from_memory(include_bytes!("../assets/icon-256.png")).ok()?.to_rgba8();
    let (width, height) = img.dimensions();
    Some(egui::IconData { rgba: img.into_raw(), width, height })
}

/// Keep favourites and the last project from the previous name of the app.
fn migrate_storage() {
    if let (Some(old), Some(new)) = (eframe::storage_dir("Luci Live"), eframe::storage_dir(APP_NAME)) {
        let (o, n) = (old.join("app.ron"), new.join("app.ron"));
        if o.exists() && !n.exists() {
            let _ = std::fs::create_dir_all(&new);
            let _ = std::fs::copy(&o, &n);
        }
    }
}
