use anyhow::{Error, Result, Context};
use image::DynamicImage;
use std::sync::Arc;
use tokio::sync::broadcast;
use tracing::{info, debug, error, warn};
use xcap::Monitor;
use std::time::{Duration, Instant};
use crate::diff::{compare_with_previous_image, MaxAverageFrame};
use crate::encode::{start_ffmpeg_process, write_frame_with_retry};
use crate::activity::ActivityMonitor;
use std::path::Path;
use chrono::Local;

// --- SafeMonitor Implementation (from screenpipe-vision) ---

#[derive(Clone)]
pub struct SafeMonitor {
    monitor_id: u32,
    monitor_data: Arc<MonitorData>,
}

#[derive(Clone)]
pub struct MonitorData {
    pub width: u32,
    pub height: u32,
    pub name: String,
    #[allow(dead_code)]
    pub is_primary: bool,
}

impl SafeMonitor {
    pub fn new(monitor: Monitor) -> Self {
        let monitor_id = monitor.id().unwrap();
        let monitor_data = Arc::new(MonitorData {
            width: monitor.width().unwrap(),
            height: monitor.height().unwrap(),
            name: monitor.name().unwrap().to_string(),
            is_primary: monitor.is_primary().unwrap(),
        });

        Self {
            monitor_id,
            monitor_data,
        }
    }

    pub async fn capture_image(&self) -> Result<DynamicImage> {
        let monitor_id = self.monitor_id;

        let image = std::thread::spawn(move || -> Result<DynamicImage> {
            let monitor = Monitor::all()
                .map_err(Error::from)?
                .into_iter()
                .find(|m| m.id().unwrap() == monitor_id)
                .ok_or_else(|| anyhow::anyhow!("Monitor not found"))?;

            if monitor.width().unwrap() == 0 || monitor.height().unwrap() == 0 {
                return Err(anyhow::anyhow!("Invalid monitor dimensions"));
            }

            monitor
                .capture_image()
                .map_err(Error::from)
                .map(DynamicImage::ImageRgba8)
        })
        .join()
        .unwrap()?;

        Ok(image)
    }

    pub fn id(&self) -> u32 {
        self.monitor_id
    }
    
    pub fn name(&self) -> &str {
        &self.monitor_data.name
    }
    
    pub fn width(&self) -> u32 {
        self.monitor_data.width
    }
    
    pub fn height(&self) -> u32 {
        self.monitor_data.height
    }
}

pub async fn list_monitors() -> Vec<SafeMonitor> {
    tokio::task::spawn_blocking(|| {
        Monitor::all()
            .unwrap_or_default()
            .into_iter()
            .map(SafeMonitor::new)
            .collect()
    })
    .await
    .unwrap_or_default()
}

pub async fn get_monitor_by_id(id: u32) -> Option<SafeMonitor> {
    tokio::task::spawn_blocking(move || match Monitor::all() {
        Ok(monitors) => monitors
            .into_iter()
            .find(|m| m.id().unwrap() == id)
            .map(SafeMonitor::new),
        Err(_) => None,
    })
    .await
    .unwrap_or(None)
}

// --- Recorder Implementation ---

pub struct Recorder {
    monitor_id: u32,
    output_dir: String,
    fps: f64,
}

impl Recorder {
    pub fn new(monitor_id: u32, output_dir: String, fps: f64) -> Self {
        Self {
            monitor_id,
            output_dir,
            fps,
        }
    }

    pub async fn run(&self, mut stop_rx: broadcast::Receiver<()>) -> Result<()> {
        info!("Starting recording for monitor {}", self.monitor_id);
        
        let monitor = get_monitor_by_id(self.monitor_id).await
            .ok_or_else(|| anyhow::anyhow!("Monitor {} not found", self.monitor_id))?;
            
        let mut frame_counter: u64 = 0;
        let mut previous_image: Option<DynamicImage> = None;
        let mut max_average: Option<MaxAverageFrame> = None;
        let mut max_avg_value = 0.0;
        
        // Ensure output directory exists
        std::fs::create_dir_all(&self.output_dir)
            .context(format!("Failed to create output directory: {}", self.output_dir))?;
        
        // Generate filename
        let timestamp = Local::now().format("%Y-%m-%d_%H-%M-%S");
        let video_filename = format!("monitor_{}_{}.mp4", self.monitor_id, timestamp);
        let video_path = Path::new(&self.output_dir).join(video_filename);
        let video_path_str = video_path.to_str().ok_or(anyhow::anyhow!("Invalid path"))?;
        
        // Activity log setup
        let log_filename = format!("monitor_{}_{}.jsonl", self.monitor_id, timestamp);
        let log_path = Path::new(&self.output_dir).join(log_filename);
        let mut activity_monitor = ActivityMonitor::new(log_path);
        
        let mut ffmpeg_child = start_ffmpeg_process(video_path_str, self.fps).await?;
        let mut ffmpeg_stdin = ffmpeg_child.stdin.take().context("Failed to get ffmpeg stdin")?;
        
        let interval = Duration::from_secs_f64(1.0 / self.fps);
        let mut next_tick = Instant::now();

        loop {
            // Check for stop signal
            if stop_rx.try_recv().is_ok() {
                info!("Stop signal received");
                break;
            }

            // Check activity and update log
            let is_allowed = activity_monitor.check_activity();

            if !is_allowed {
                debug!("Capture blocked due to restricted activity");
                // Skip capture, but sleep to maintain loop timing
                // We do NOT write to ffmpeg here (VFR behavior)
                // Log is updated inside check_activity
            } else {
                // Capture
                match monitor.capture_image().await {
                    Ok(image) => {
                        // Diff
                        let current_average = compare_with_previous_image(
                            previous_image.as_ref(),
                            &image,
                            &mut max_average,
                            frame_counter,
                            &mut max_avg_value,
                        ).unwrap_or(1.0); // Default to changed if diff fails
                        
                        // Force first frame or if diff is significant
                        let should_write = previous_image.is_none() || current_average >= 0.006;
                        
                        if should_write {
                            // Write to FFmpeg
                            if let Err(e) = write_frame_with_retry(&mut ffmpeg_stdin, &image).await {
                                error!("Failed to write frame: {}", e);
                                break; // Stop on write error
                            }
                            previous_image = Some(image);
                            frame_counter += 1;
                            debug!("Frame {} written (diff: {:.4})", frame_counter, current_average);
                        } else {
                            debug!("Skipping frame {} (diff: {:.4})", frame_counter, current_average);
                            frame_counter += 1;
                        }
                    },
                    Err(e) => {
                        warn!("Failed to capture image: {}", e);
                    }
                }
            }

            // Sleep logic
            next_tick += interval;
            let now = Instant::now();
            if next_tick > now {
                tokio::time::sleep(next_tick - now).await;
            } else {
                // We are behind, reset next_tick to avoid burst
                next_tick = now;
            }
        }
        
        // Flush final log entry
        activity_monitor.flush();
        
        // Cleanup FFmpeg
        drop(ffmpeg_stdin); // Close stdin to signal EOF
        match ffmpeg_child.wait().await {
            Ok(status) => info!("FFmpeg finished with status: {}", status),
            Err(e) => error!("Failed to wait for FFmpeg: {}", e),
        }
        
        Ok(())
    }
}
