//! Opt-in coarse engine timings (`ANIMATHIO_EXPORT_TIMING=1`).
use std::{
    collections::BTreeMap,
    sync::{Mutex, OnceLock},
    time::{Duration, Instant},
};
static TOTALS: Mutex<BTreeMap<&'static str, Duration>> = Mutex::new(BTreeMap::new());
static ENABLED: OnceLock<bool> = OnceLock::new();
/// Scoped wall timer. Nested stages are inclusive; worker times can overlap.
pub struct Timer(Option<(&'static str, Instant)>);
impl Timer {
    /// Start a named stage when profiling is enabled.
    pub fn new(stage: &'static str) -> Self {
        Self(
            (*ENABLED.get_or_init(|| std::env::var_os("ANIMATHIO_EXPORT_TIMING").is_some()))
                .then(|| (stage, Instant::now())),
        )
    }
}
impl Drop for Timer {
    fn drop(&mut self) {
        if let Some((stage, start)) = self.0 {
            *TOTALS
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .entry(stage)
                .or_default() += start.elapsed();
        }
    }
}
/// Print accumulated process-wide times. Intended for a single headless job.
pub fn report() {
    for (stage, elapsed) in TOTALS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .iter()
    {
        eprintln!("export timing {stage}: {:.3}s", elapsed.as_secs_f64());
    }
}
