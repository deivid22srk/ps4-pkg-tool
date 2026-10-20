use std::sync::atomic::{AtomicBool, Ordering};

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
        }
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
        if pct != self.last_percent {
            self.last_percent = pct;
            self.progress.report(pct, &self.label)?;
        }
        Ok(())
    }

    pub fn finish(&mut self) -> Result<()> {
        self.progress.report(self.end, &self.label)
    }
}
