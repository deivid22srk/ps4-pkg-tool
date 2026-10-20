use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use crate::error::{CoreError, Result};

static CANCELLED: AtomicBool = AtomicBool::new(false);

pub fn reset_cancel() {
    CANCELLED.store(false, Ordering::SeqCst);
}

pub fn request_cancel() {
    CANCELLED.store(true, Ordering::SeqCst);
}

pub fn is_cancelled() -> bool {
    CANCELLED.load(Ordering::SeqCst)
}

/// Receptor de progresso (0..=100) e ponto de checagem de cancelamento.
pub trait Progress {
    fn report(&mut self, percent: u8, message: &str) -> Result<()>;
}

pub struct NoopProgress;

impl Progress for NoopProgress {
    fn report(&mut self, _percent: u8, _message: &str) -> Result<()> {
        if is_cancelled() {
            Err(CoreError::Cancelled)
        } else {
            Ok(())
        }
    }
}

/// Converte bytes processados em progresso dentro de uma faixa [start, end].
pub struct ByteProgress<'a> {
    progress: &'a mut dyn Progress,
    start: u8,
    end: u8,
    total: u64,
    done: u64,
    label: String,
    last_percent: u8,
    started: Instant,
    last_emit: Instant,
}

impl<'a> ByteProgress<'a> {
    pub fn new(
        progress: &'a mut dyn Progress,
        start: u8,
        end: u8,
        total: u64,
        label: impl Into<String>,
    ) -> Self {
        Self {
            progress,
            start,
            end,
            total,
            done: 0,
            label: label.into(),
            last_percent: 255,
            started: Instant::now(),
            last_emit: Instant::now(),
        }
    }

    fn decorated(&self) -> String {
        let elapsed = self.started.elapsed().as_secs_f64().max(0.001);
        let speed = self.done as f64 / elapsed;
        let remaining = self.total.saturating_sub(self.done.min(self.total));
        let eta = if speed > 0.01 { remaining as f64 / speed } else { 0.0 };
        format!(
            "{} • {}/s • ETA {}",
            self.label,
            format_speed(speed),
            format_eta(eta)
        )
    }

    pub fn add(&mut self, bytes: u64) -> Result<()> {
        if is_cancelled() {
            return Err(CoreError::Cancelled);
        }
        self.done = self.done.saturating_add(bytes);
        let pct = if self.total == 0 {
            self.end
        } else {
            let span = self.end.saturating_sub(self.start) as u64;
            let value = self.start as u64 + (self.done.min(self.total) * span) / self.total;
            value.min(100) as u8
        };
        let now = Instant::now();
        if pct != self.last_percent || now.duration_since(self.last_emit).as_millis() >= 700 {
            self.last_percent = pct;
            self.last_emit = now;
            let message = self.decorated();
            self.progress.report(pct, &message)?;
        }
        Ok(())
    }

    pub fn finish(&mut self) -> Result<()> {
        let message = self.decorated();
        self.progress.report(self.end, &message)
    }
}

pub(crate) fn format_speed(bytes_per_second: f64) -> String {
    if !bytes_per_second.is_finite() || bytes_per_second < 0.0 {
        return "—".into();
    }
    if bytes_per_second >= 1_000_000_000.0 {
        format!("{:.1} GB", bytes_per_second / 1_000_000_000.0)
    } else if bytes_per_second >= 1_000_000.0 {
        format!("{:.1} MB", bytes_per_second / 1_000_000.0)
    } else if bytes_per_second >= 1_000.0 {
        format!("{:.1} KB", bytes_per_second / 1_000.0)
    } else {
        format!("{:.0} B", bytes_per_second)
    }
}

pub(crate) fn format_eta(seconds: f64) -> String {
    if !seconds.is_finite() || seconds <= 0.0 {
        return "—".into();
    }
    let total = seconds.ceil() as u64;
    if total >= 3600 {
        format!("{}h{:02}m", total / 3600, (total % 3600) / 60)
    } else if total >= 60 {
        format!("{}m{:02}s", total / 60, total % 60)
    } else {
        format!("{total}s")
    }
}
