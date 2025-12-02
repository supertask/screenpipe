use eframe::egui;
use tokio::sync::broadcast;
use tracing::{info, error, Level};
use tracing_subscriber::FmtSubscriber;
use std::path::PathBuf;
use crate::capture::{Recorder, list_monitors, SafeMonitor};

mod capture;
mod encode;
mod diff;
mod activity; // 追加

fn main() -> eframe::Result<()> {
    // Setup logging
    let subscriber = FmtSubscriber::builder()
        .with_max_level(Level::DEBUG)
        .finish();
    tracing::subscriber::set_global_default(subscriber)
        .expect("setting default subscriber failed");

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([400.0, 300.0]),
        ..Default::default()
    };

    eframe::run_native(
        "Screenpipe Prototype 1",
        options,
        Box::new(|_cc| Ok(Box::new(MyApp::new()))),
    )
}

struct MyApp {
    monitors: Vec<SafeMonitor>,
    is_recording: bool,
    stop_tx: Option<broadcast::Sender<()>>,
    rt: tokio::runtime::Runtime,
    status: String,
}

impl MyApp {
    fn new() -> Self {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let monitors = rt.block_on(list_monitors());
        
        Self {
            monitors,
            is_recording: false,
            stop_tx: None,
            rt,
            status: "Ready".to_string(),
        }
    }
}

impl eframe::App for MyApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("Screenpipe Prototype 1");

            ui.separator();

            ui.label(format!("Detected Monitors: {}", self.monitors.len()));
            for m in &self.monitors {
                ui.label(format!(" - {} ({}x{})", m.name(), m.width(), m.height()));
            }

            if ui.button("Refresh Monitors").clicked() {
                self.monitors = self.rt.block_on(list_monitors());
            }

            ui.separator();

            if self.is_recording {
                ui.label(format!("Status: Recording... {}", self.status));
                if ui.button("Stop Recording").clicked() {
                    if let Some(tx) = &self.stop_tx {
                        let _ = tx.send(());
                    }
                    self.is_recording = false;
                    self.stop_tx = None;
                    self.status = "Stopped".to_string();
                }
            } else {
                ui.label(format!("Status: {}", self.status));
                let can_start = !self.monitors.is_empty();
                if ui.add_enabled(can_start, egui::Button::new("Start Recording")).clicked() {
                    // output dir is $HOME/.work_recorder
                    let output_dir = dirs::home_dir()
                        .map(|p| p.join(".work_recorder"))
                        .unwrap_or_else(|| PathBuf::from(".work_recorder"))
                        .to_string_lossy()
                        .to_string();
                    let fps = 1.0;
                    
                    let (tx, _rx) = broadcast::channel(1);
                    self.stop_tx = Some(tx.clone());
                    
                    // Start recording for ALL monitors simultaneously
                    for monitor in &self.monitors {
                        let monitor_id = monitor.id();
                        let output_dir_clone = output_dir.clone();
                        let stop_rx = tx.subscribe(); // Each recorder gets a subscriber
                        
                        let recorder = Recorder::new(monitor_id, output_dir_clone, fps);
                        
                        self.rt.spawn(async move {
                            match recorder.run(stop_rx).await {
                                Ok(_) => info!("Recording finished successfully for monitor {}", monitor_id),
                                Err(e) => error!("Recording failed for monitor {}: {}", monitor_id, e),
                            }
                        });
                    }
                    
                    self.is_recording = true;
                    self.status = format!("Recording {} monitor(s)", self.monitors.len());
                }
            }
            
            ui.separator();
            ui.label("Check console for detailed logs.");
        });
    }
}
