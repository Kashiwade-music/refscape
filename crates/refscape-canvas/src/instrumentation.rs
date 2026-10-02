//! Opt-in deterministic work counters for the performance lane.
//! Geometry inputs contain no source body; counters measure work performed on them.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct WorkCounters {
    pub overlap_checks: u64,
    pub cancellation_checkpoints: u64,
    pub nearest_candidates: u64,
    pub cached_metric_reads: u64,
}

#[cfg(feature = "work-counters")]
thread_local! {
    static COUNTERS: std::cell::Cell<WorkCounters> = const { std::cell::Cell::new(WorkCounters {
        overlap_checks:0,cancellation_checkpoints:0,nearest_candidates:0,cached_metric_reads:0
    }) };
}

#[cfg(feature = "work-counters")]
pub fn reset() {
    COUNTERS.with(|value| value.set(WorkCounters::default()));
}
#[cfg(feature = "work-counters")]
pub fn counters() -> WorkCounters {
    COUNTERS.with(std::cell::Cell::get)
}
#[cfg(not(feature = "work-counters"))]
pub fn reset() {}
#[cfg(not(feature = "work-counters"))]
pub fn counters() -> WorkCounters {
    WorkCounters::default()
}

#[inline]
pub(crate) fn overlap() {
    #[cfg(feature = "work-counters")]
    update(|value| value.overlap_checks += 1);
}
#[inline]
pub(crate) fn checkpoint() {
    #[cfg(feature = "work-counters")]
    update(|value| value.cancellation_checkpoints += 1);
}
#[inline]
pub(crate) fn candidate() {
    #[cfg(feature = "work-counters")]
    update(|value| value.nearest_candidates += 1);
}
#[inline]
pub(crate) fn metrics() {
    #[cfg(feature = "work-counters")]
    update(|value| value.cached_metric_reads += 1);
}

#[cfg(feature = "work-counters")]
fn update(change: impl FnOnce(&mut WorkCounters)) {
    COUNTERS.with(|value| {
        let mut counters = value.get();
        change(&mut counters);
        value.set(counters);
    });
}
