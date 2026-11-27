//! DMX output thread: renders the show at a fixed rate and sends it to a
//! USB-DMX interface (FTDI "Open DMX" style dongles or Enttec Pro compatible).
//! Rendering happens here, not in the window, so the show keeps running
//! even when the window is minimized.

use crate::engine;
use crate::live::SharedHandle;
use crate::model::DmxProtocol;
use std::io::Write;
use std::time::{Duration, Instant};

pub struct PortInfo {
    pub name: String,
    pub desc: String,
}

pub fn ports() -> Vec<PortInfo> {
    match serialport::available_ports() {
        Ok(v) => v
            .into_iter()
            .map(|p| {
                let desc = match &p.port_type {
                    serialport::SerialPortType::UsbPort(u) => format!(
                        "USB {}{}",
                        u.product.clone().unwrap_or_default(),
                        u.manufacturer.as_ref().map(|m| format!(" ({m})")).unwrap_or_default()
                    ),
                    serialport::SerialPortType::BluetoothPort => "Bluetooth".into(),
                    serialport::SerialPortType::PciPort => "PCI".into(),
                    serialport::SerialPortType::Unknown => String::new(),
                };
                PortInfo { name: p.port_name, desc }
            })
            .collect(),
        Err(_) => vec![],
    }
}

fn open(name: &str, proto: DmxProtocol) -> Result<Box<dyn serialport::SerialPort>, String> {
    let baud = match proto {
        DmxProtocol::OpenDmx => 250_000,
        DmxProtocol::EnttecPro => 57_600,
    };
    serialport::new(name, baud)
        .data_bits(serialport::DataBits::Eight)
        .stop_bits(serialport::StopBits::Two)
        .parity(serialport::Parity::None)
        .flow_control(serialport::FlowControl::None)
        .timeout(Duration::from_millis(200))
        .open()
        .map_err(|e| format!("Impossibile aprire {name}: {e}"))
}

fn send(port: &mut Box<dyn serialport::SerialPort>, proto: DmxProtocol, uni: &[u8; 512]) -> Result<(), String> {
    match proto {
        DmxProtocol::OpenDmx => {
            port.set_break().map_err(|e| e.to_string())?;
            std::thread::sleep(Duration::from_micros(150));
            port.clear_break().map_err(|e| e.to_string())?;
            std::thread::sleep(Duration::from_micros(20));
            let mut frame = Vec::with_capacity(513);
            frame.push(0u8);
            frame.extend_from_slice(uni);
            port.write_all(&frame).map_err(|e| e.to_string())?;
        }
        DmxProtocol::EnttecPro => {
            let len = 513u16;
            let mut frame = Vec::with_capacity(518);
            frame.extend([0x7E, 6, (len & 0xFF) as u8, (len >> 8) as u8, 0]);
            frame.extend_from_slice(uni);
            frame.push(0xE7);
            port.write_all(&frame).map_err(|e| e.to_string())?;
        }
    }
    port.flush().map_err(|e| e.to_string())
}

pub fn spawn(shared: SharedHandle) {
    std::thread::Builder::new()
        .name("dmx-output".into())
        .spawn(move || {
            let catalog = crate::catalog::builtin();
            let mut port: Option<(String, DmxProtocol, Box<dyn serialport::SerialPort>)> = None;
            let mut retry_at = Instant::now();
            loop {
                let frame_start = Instant::now();
                let (project, t, song_idx, blackout, identify, want, proto, fps, test, manual) = {
                    let mut s = match shared.lock() {
                        Ok(s) => s,
                        Err(_) => return,
                    };
                    let ident = match &s.identify {
                        Some((ids, at)) if at.elapsed() < Duration::from_secs(3) => Some(ids.clone()),
                        Some(_) => {
                            s.identify = None;
                            None
                        }
                        None => None,
                    };
                    (
                        s.project.clone(),
                        s.transport.pos(),
                        s.transport.song,
                        s.blackout,
                        ident,
                        s.dmx.port.clone(),
                        s.dmx.protocol,
                        s.dmx.fps.clamp(10, 44),
                        s.test_channels.clone(),
                        s.manual.clone(),
                    )
                };
                let song = project.songs.get(song_idx);
                // blackout and "identify" win over the hand-set channels
                let plain = !blackout && identify.is_none();
                let states = engine::render(&engine::RenderInput {
                    project: &project,
                    catalog: &catalog,
                    song,
                    t,
                    blackout,
                    identify,
                });
                let mut uni = engine::universe(&project, &catalog, &states);
                if plain {
                    engine::apply_manual(&project, &catalog, &manual, &mut uni);
                }
                if let Some(test) = test {
                    uni = [0; 512];
                    for (c, v) in test {
                        if c < 512 {
                            uni[c] = v;
                        }
                    }
                }

                // open / close the port when the configuration changes
                let mut status: Option<String> = None;
                match (&want, &port) {
                    (None, Some(_)) => {
                        port = None;
                        status = Some("Uscita DMX non collegata".into());
                    }
                    (Some(w), Some((n, p, _))) if w != n || *p != proto => {
                        port = None;
                    }
                    _ => {}
                }
                if port.is_none() {
                    if let Some(w) = &want {
                        if Instant::now() >= retry_at {
                            match open(w, proto) {
                                Ok(p) => {
                                    status = Some(format!("Collegato a {w} ({})", proto.label()));
                                    port = Some((w.clone(), proto, p));
                                }
                                Err(e) => {
                                    status = Some(e);
                                    retry_at = Instant::now() + Duration::from_secs(2);
                                }
                            }
                        }
                    }
                }
                let mut sent = false;
                if let Some((n, pr, p)) = port.as_mut() {
                    match send(p, *pr, &uni) {
                        Ok(()) => sent = true,
                        Err(e) => {
                            status = Some(format!("Errore su {n}: {e}"));
                            port = None;
                            retry_at = Instant::now() + Duration::from_secs(2);
                        }
                    }
                }
                if let Ok(mut s) = shared.lock() {
                    s.universe = uni;
                    if sent {
                        s.dmx_frames += 1;
                    }
                    if let Some(st) = status {
                        s.dmx_status = st;
                    }
                }
                let period = Duration::from_secs_f64(1.0 / fps as f64);
                let el = frame_start.elapsed();
                if el < period {
                    std::thread::sleep(period - el);
                }
            }
        })
        .ok();
}
