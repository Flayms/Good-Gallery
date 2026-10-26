//! Scripted scroll with frame statistics, written to `results/<name>.json`.

use crate::Args;
use crate::stream::Streamer;
use eframe::egui;
use serde_json::json;
use std::time::Instant;

enum Phase {
    /// Waits until the first screen is loaded (or 3 s), so the run measures scrolling, not startup.
    Warmup(Instant),
    Scrolling(Instant),
    Done,
}

pub struct Bench {
    name: String,
    config: serde_json::Value,
    speed: f32,
    seconds: f32,
    phase: Phase,
    last_placeholders: usize,
    first_screen_ms: Option<f32>,
    created: Instant,
    intervals: Vec<f32>,
    cpu: Vec<f32>,
    placeholder_rate: Vec<f32>,
    decode_from: usize,
    upload_from: usize,
    /// Set while the files the run will scroll past are read once, so Defender's first-open scan is out of the way.
    pub warming: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
    pub warm: bool,
}

fn pct(sorted: &[f32], p: f32) -> f32 {
    if sorted.is_empty() {
        return 0.0;
    }
    sorted[((sorted.len() - 1) as f32 * p).round() as usize]
}

fn summary(mut v: Vec<f32>) -> serde_json::Value {
    v.sort_by(f32::total_cmp);
    let mean = v.iter().sum::<f32>() / v.len().max(1) as f32;
    json!({ "n": v.len(), "mean": mean, "p50": pct(&v, 0.5), "p95": pct(&v, 0.95), "p99": pct(&v, 0.99), "max": v.last().copied().unwrap_or(0.0) })
}

impl Bench {
    pub fn new(args: &Args, scan_ms: f32, threads: usize) -> Self {
        Self {
            name: args.str("bench", "run"),
            config: json!({
                "items": args.get("items", 100_000usize),
                "view": args.str("view", "justified"),
                "cols": args.get("cols", 5usize),
                "vsync": args.str("vsync", "on"),
                "decoder": args.str("decoder", "libwebp"),
                "upload": args.str("upload", "native"),
                "glass": args.str("glass", "off"),
                "warm": args.str("warm", "on"),
                "threads": threads,
                "budget_mb": args.get("budget-mb", 512usize),
                "upload_budget_ms": args.get("upload-budget-ms", 3.0f32),
                "speed": args.get("speed", 1440.0f32),
                "scan_ms": scan_ms,
            }),
            speed: args.get("speed", 1440.0),
            seconds: args.get("seconds", 20.0),
            phase: Phase::Warmup(Instant::now()),
            last_placeholders: usize::MAX,
            first_screen_ms: None,
            created: Instant::now(),
            intervals: Vec::new(),
            cpu: Vec::new(),
            placeholder_rate: Vec::new(),
            decode_from: 0,
            upload_from: 0,
            warming: None,
            warm: args.str("warm", "on") == "on",
        }
    }

    /// Pixels the run scrolls.
    pub fn distance(&self) -> f32 {
        self.speed * self.seconds
    }

    pub fn advance(&mut self, scroll: &mut f32, interval: Option<f32>, ctx: &egui::Context) {
        ctx.request_repaint();
        match self.phase {
            Phase::Warmup(_)
                if self
                    .warming
                    .as_ref()
                    .is_some_and(|w| !w.load(std::sync::atomic::Ordering::Relaxed)) =>
            {
                self.phase = Phase::Warmup(Instant::now());
            }
            Phase::Warmup(start) => {
                if self.last_placeholders == 0 && self.first_screen_ms.is_none() {
                    self.first_screen_ms = Some(self.created.elapsed().as_secs_f32() * 1000.0);
                }
                if (self.last_placeholders == 0 && start.elapsed().as_secs_f32() > 1.5)
                    || start.elapsed().as_secs_f32() > 3.0
                {
                    self.phase = Phase::Scrolling(Instant::now());
                }
            }
            Phase::Scrolling(start) => {
                *scroll += self.speed * interval.unwrap_or(0.0).min(0.1);
                if start.elapsed().as_secs_f32() >= self.seconds {
                    self.phase = Phase::Done;
                }
            }
            Phase::Done => {}
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn record(
        &mut self,
        interval: Option<f32>,
        cpu_ms: f32,
        shown: usize,
        placeholders: usize,
        streamer: &Streamer,
        layout_ms: f32,
        ctx: &egui::Context,
    ) {
        self.last_placeholders = placeholders;
        match self.phase {
            Phase::Warmup(_) => {
                self.decode_from = streamer.stats.work_ms.len();
                self.upload_from = streamer.stats.upload_ms.len();
            }
            Phase::Scrolling(_) => {
                if let Some(i) = interval {
                    self.intervals.push(i * 1000.0);
                }
                self.cpu.push(cpu_ms);
                self.placeholder_rate
                    .push(placeholders as f32 / shown.max(1) as f32);
            }
            Phase::Done => {
                self.write(streamer, layout_ms);
                self.phase = Phase::Done;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                // Only write once.
                self.intervals.clear();
            }
        }
    }

    fn write(&self, streamer: &Streamer, layout_ms: f32) {
        if self.intervals.is_empty() {
            return;
        }
        let over = |ms: f32| {
            self.intervals.iter().filter(|&&i| i > ms).count() as f32 * 1000.0
                / self.intervals.len() as f32
        };
        let rss = memory_stats::memory_stats().map_or(0, |m| m.physical_mem) >> 20;
        let result = json!({
            "name": self.name,
            "config": self.config,
            "first_screen_ms": self.first_screen_ms,
            "layout_ms": layout_ms,
            "interval_ms": summary(self.intervals.clone()),
            "cpu_ms": summary(self.cpu.clone()),
            "over_20ms_per_1000": over(20.0),
            "over_33ms_per_1000": over(33.0),
            "placeholder_rate": summary(self.placeholder_rate.clone()),
            "worker_ms": summary(streamer.stats.work_ms[self.decode_from..].to_vec()),
            "read_ms": summary(streamer.stats.read_ms[self.decode_from..].to_vec()),
            "upload_ms": summary(streamer.stats.upload_ms[self.upload_from..].to_vec()),
            "texture_mb": streamer.texture_bytes() >> 20,
            "rss_mb": rss,
        });
        std::fs::create_dir_all("results").unwrap();
        let path = format!("results/{}.json", self.name);
        std::fs::write(&path, serde_json::to_string_pretty(&result).unwrap()).unwrap();
        println!("{}", serde_json::to_string(&result).unwrap());
    }
}
