//! `horizon-ctl record`: the finished frames (post passes and the HUD
//! included) streamed to an ffmpeg process as a video at a steady frame rate.
//!
//! Each frame is copied into a buffer the GPU fills while the next frames
//! draw; up to three are on their way back at once, so the game does not wait
//! on the copy. A writer thread feeds ffmpeg, repeating or dropping frames by
//! the time they were drawn so the video plays at the game's own speed.
use std::{
    collections::VecDeque,
    io::Write,
    path::Path,
    process::{Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicU8, Ordering},
        mpsc,
    },
    thread,
    time::Instant,
};

/// Frames copied but not yet read back, at most.
const IN_FLIGHT: usize = 3;
/// Frames read back and waiting for ffmpeg, at most (about 15 MB each at 1440p).
const QUEUE: usize = 10;

pub struct Recorder {
    size: [u32; 2],
    padded_row: u32,
    started: Instant,
    in_flight: VecDeque<Pending>,
    spare: Vec<wgpu::Buffer>,
    frames: Option<mpsc::SyncSender<(f64, Vec<u8>)>>,
    writer: Option<thread::JoinHandle<Result<u64, String>>>,
}

struct Pending {
    buffer: wgpu::Buffer,
    /// Seconds since the recording started that it was drawn.
    at: f64,
    /// 0 copying, 1 mapped, 2 failed; `None` until its copy is submitted.
    state: Option<Arc<AtomicU8>>,
}

impl Recorder {
    /// Start ffmpeg writing `path`. `bgra` is the surface's byte order.
    pub fn start(
        path: &Path,
        size: [u32; 2],
        padded_row: u32,
        bgra: bool,
        fps: f64,
    ) -> Result<Self, String> {
        // NVENC takes the frames as they are and converts them on the GPU; libx264 needs ffmpeg to convert them on the
        // CPU first, which at 1440p60 is what falls behind.
        let encoder = if ffmpeg_has("h264_nvenc") {
            vec![
                "-c:v",
                "h264_nvenc",
                "-preset",
                "p4",
                "-rc",
                "vbr",
                "-cq",
                "19",
                "-b:v",
                "0",
                "-profile:v",
                "high",
            ]
        } else {
            vec![
                "-c:v", "libx264", "-preset", "veryfast", "-crf", "18", "-pix_fmt", "yuv420p",
            ]
        };
        let dims = format!("{}x{}", size[0], size[1]);
        let rate = format!("{fps}");
        let mut child = Command::new("ffmpeg")
            .args(["-loglevel", "error", "-y", "-f", "rawvideo"])
            .args(["-pix_fmt", if bgra { "bgra" } else { "rgba" }])
            .args(["-s", &dims, "-r", &rate, "-i", "pipe:0"])
            .args(&encoder)
            .args(["-movflags", "+faststart"])
            .arg(path)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|e| format!("could not start ffmpeg: {e}"))?;
        let mut stdin = child.stdin.take().ok_or("ffmpeg has no stdin")?;
        let (frames, received) = mpsc::sync_channel::<(f64, Vec<u8>)>(QUEUE);
        let writer = thread::Builder::new()
            .name("Horizon recorder".into())
            .spawn(move || {
                let mut written = 0u64;
                for (at, frame) in received {
                    // every video frame due by the time this one was drawn
                    let due = (at * fps).floor() as u64 + 1;
                    while written < due {
                        stdin
                            .write_all(&frame)
                            .map_err(|e| format!("ffmpeg stopped taking frames: {e}"))?;
                        written += 1;
                    }
                }
                drop(stdin);
                let status = child.wait().map_err(|e| e.to_string())?;
                if status.success() {
                    Ok(written)
                } else {
                    Err(format!("ffmpeg exited with {status}"))
                }
            })
            .map_err(|e| e.to_string())?;
        Ok(Self {
            size,
            padded_row,
            started: Instant::now(),
            in_flight: VecDeque::new(),
            spare: Vec::new(),
            frames: Some(frames),
            writer: Some(writer),
        })
    }

    pub fn size(&self) -> [u32; 2] {
        self.size
    }

    /// Before the frame is submitted: copy the finished picture out of `texture`.
    pub fn capture(
        &mut self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        texture: &wgpu::Texture,
    ) {
        let buffer = self.spare.pop().unwrap_or_else(|| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("Horizon recording"),
                size: u64::from(self.padded_row) * u64::from(self.size[1]),
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            })
        });
        encoder.copy_texture_to_buffer(
            texture.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(self.padded_row),
                    rows_per_image: None,
                },
            },
            wgpu::Extent3d {
                width: self.size[0],
                height: self.size[1],
                depth_or_array_layers: 1,
            },
        );
        let at = self.started.elapsed().as_secs_f64();
        self.in_flight.push_back(Pending {
            buffer,
            at,
            state: None,
        });
    }

    /// After the frame is submitted: start reading it back, and pass on every frame that has arrived.
    pub fn collect(&mut self, device: &wgpu::Device) {
        if let Some(newest) = self.in_flight.back_mut()
            && newest.state.is_none()
        {
            let state = Arc::new(AtomicU8::new(0));
            let flag = state.clone();
            newest.buffer.map_async(wgpu::MapMode::Read, .., move |r| {
                flag.store(if r.is_ok() { 1 } else { 2 }, Ordering::Release);
            });
            newest.state = Some(state);
        }
        let _ = device.poll(wgpu::PollType::Poll);
        while let Some(front) = self.in_flight.front() {
            let state = front
                .state
                .as_ref()
                .map_or(0, |s| s.load(Ordering::Acquire));
            if state == 0 {
                if self.in_flight.len() <= IN_FLIGHT {
                    break;
                }
                // too far behind: wait for the oldest rather than pile up more
                let _ = device.poll(wgpu::PollType::wait_indefinitely());
                continue;
            }
            let Some(done) = self.in_flight.pop_front() else {
                break;
            };
            if state == 1 {
                self.send(&done);
                done.buffer.unmap();
            }
            self.spare.push(done.buffer);
        }
    }

    fn send(&self, done: &Pending) {
        let Some(frames) = &self.frames else {
            return;
        };
        let Ok(data) = done.buffer.get_mapped_range(..) else {
            return;
        };
        let row = self.size[0] as usize * 4;
        let mut frame = Vec::with_capacity(row * self.size[1] as usize);
        for y in 0..self.size[1] as usize {
            let start = y * self.padded_row as usize;
            frame.extend_from_slice(&data[start..start + row]);
        }
        drop(data);
        // If ffmpeg falls behind the frame is dropped: the writer repeats the next one to keep time.
        let _ = frames.try_send((done.at, frame));
    }

    /// Read back what is still on its way, close the file and wait for ffmpeg. Returns the frames written.
    pub fn finish(mut self, device: &wgpu::Device) -> Result<u64, String> {
        while !self.in_flight.is_empty() {
            if self.in_flight.back().is_some_and(|p| p.state.is_none()) {
                self.collect(device);
            }
            let _ = device.poll(wgpu::PollType::wait_indefinitely());
            let Some(done) = self.in_flight.pop_front() else {
                break;
            };
            if done
                .state
                .as_ref()
                .is_some_and(|s| s.load(Ordering::Acquire) == 1)
            {
                self.send(&done);
                done.buffer.unmap();
            }
        }
        self.frames = None;
        self.writer
            .take()
            .ok_or("no writer")?
            .join()
            .map_err(|_| "the recorder thread panicked".to_owned())?
    }
}

/// Whether this ffmpeg has an encoder.
fn ffmpeg_has(encoder: &str) -> bool {
    Command::new("ffmpeg")
        .args(["-hide_banner", "-encoders"])
        .stderr(Stdio::null())
        .output()
        .is_ok_and(|o| String::from_utf8_lossy(&o.stdout).contains(encoder))
}
