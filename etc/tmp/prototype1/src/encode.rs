use anyhow::{Result, Context};
use std::process::Stdio;
use tokio::process::{Child, Command, ChildStdin};
use tokio::io::AsyncWriteExt;
use tracing::{info, warn, debug};
use std::time::Duration;
use image::DynamicImage;
use std::io::Cursor;
use image::ImageFormat;

#[allow(dead_code)]
pub struct CaptureResult {
    pub image: DynamicImage,
    pub frame_number: u64,
}

#[cfg(windows)]
const FFMPEG_EXE: &str = "ffmpeg.exe";
#[cfg(not(windows))]
const FFMPEG_EXE: &str = "ffmpeg";

pub fn find_ffmpeg_path() -> Option<String> {
    // 1. Check in project directory (etc/tmp/prototype1/ffmpeg/)
    if let Ok(exe_path) = std::env::current_exe() {
        if let Some(exe_dir) = exe_path.parent() {
            // Try to find project root by looking for Cargo.toml
            let mut current = exe_dir;
            loop {
                let ffmpeg_in_project = current.join("ffmpeg").join(FFMPEG_EXE);
                if ffmpeg_in_project.exists() {
                    debug!("Found ffmpeg in project directory: {:?}", ffmpeg_in_project);
                    return ffmpeg_in_project.to_str().map(|s| s.to_string());
                }
                
                // Also check in same directory as executable
                let ffmpeg_in_exe_dir = current.join(FFMPEG_EXE);
                if ffmpeg_in_exe_dir.exists() {
                    debug!("Found ffmpeg in executable directory: {:?}", ffmpeg_in_exe_dir);
                    return ffmpeg_in_exe_dir.to_str().map(|s| s.to_string());
                }
                
                if let Some(parent) = current.parent() {
                    current = parent;
                } else {
                    break;
                }
            }
        }
    }
    
    // 2. Check in current working directory
    if let Ok(cwd) = std::env::current_dir() {
        let ffmpeg_in_cwd = cwd.join("ffmpeg").join(FFMPEG_EXE);
        if ffmpeg_in_cwd.exists() {
            debug!("Found ffmpeg in current directory: {:?}", ffmpeg_in_cwd);
            return ffmpeg_in_cwd.to_str().map(|s| s.to_string());
        }
        
        let ffmpeg_direct = cwd.join(FFMPEG_EXE);
        if ffmpeg_direct.exists() {
            debug!("Found ffmpeg directly in current directory: {:?}", ffmpeg_direct);
            return ffmpeg_direct.to_str().map(|s| s.to_string());
        }
    }
    
    // 3. Check in PATH as fallback
    #[cfg(windows)]
    {
        if let Ok(path) = which::which(FFMPEG_EXE) {
            debug!("Found ffmpeg in PATH: {:?}", path);
            return path.to_str().map(|s| s.to_string());
        }
    }
    #[cfg(not(windows))]
    {
        if let Ok(path) = which::which(FFMPEG_EXE) {
            debug!("Found ffmpeg in PATH: {:?}", path);
            return path.to_str().map(|s| s.to_string());
        }
    }
    
    warn!("FFmpeg not found. Please place ffmpeg binary in project directory (ffmpeg/{} or same directory as executable)", FFMPEG_EXE);
    None
}

pub async fn start_ffmpeg_process(output_file: &str, fps: f64) -> Result<Child> {
    let ffmpeg_path = find_ffmpeg_path().context("FFmpeg not found")?;
    info!("Starting FFmpeg process for file: {}", output_file);
    
    let fps_str = fps.to_string();
    let mut command = Command::new(ffmpeg_path);
    let args = vec![
        "-f", "image2pipe",
        "-vcodec", "png",
        "-r", &fps_str,
        "-i", "-",
        "-vf", "pad=width=ceil(iw/2)*2:height=ceil(ih/2)*2",
        "-vcodec", "libx265",
        "-tag:v", "hvc1",
        "-preset", "ultrafast",
        "-crf", "23",
        "-pix_fmt", "yuv420p",
        output_file
    ];

    command
        .args(&args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    debug!("FFmpeg command: {:?}", command);
    let child = command.spawn().context("Failed to spawn ffmpeg")?;
    debug!("FFmpeg process spawned");
    Ok(child)
}

pub async fn write_frame_to_ffmpeg(
    stdin: &mut ChildStdin,
    image: &DynamicImage,
) -> Result<()> {
    let mut buffer = Vec::new();
    image.write_to(&mut Cursor::new(&mut buffer), ImageFormat::Png)
        .context("Failed to encode frame to PNG")?;

    stdin.write_all(&buffer).await.context("Failed to write frame to ffmpeg stdin")?;
    Ok(())
}

pub async fn write_frame_with_retry(
    stdin: &mut ChildStdin,
    image: &DynamicImage,
) -> Result<()> {
    const MAX_RETRIES: usize = 3;
    const RETRY_DELAY: Duration = Duration::from_millis(100);

    let mut retries = 0;
    while retries < MAX_RETRIES {
        match write_frame_to_ffmpeg(stdin, image).await {
            Ok(_) => return Ok(()),
            Err(e) => {
                retries += 1;
                if retries >= MAX_RETRIES {
                    return Err(anyhow::anyhow!("Failed to write frame to ffmpeg: {}", e));
                } else {
                    warn!("Failed to write frame to ffmpeg (attempt {}): {}. Retrying...", retries, e);
                    tokio::time::sleep(RETRY_DELAY).await;
                }
            }
        }
    }
    Err(anyhow::anyhow!("Failed to write frame to ffmpeg after max retries"))
}

