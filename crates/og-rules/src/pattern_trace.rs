use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, Ordering};

static ENABLED: AtomicBool = AtomicBool::new(false);

thread_local! {
    static EVENTS: RefCell<Vec<TraceEvent>> = const { RefCell::new(Vec::new()) };
}

#[derive(Debug, Clone)]
pub struct TraceEvent {
    pub pattern_idx: usize,
    pub token_idx: usize,
    pub token: String,
    pub cause: String,
}

pub fn set_enabled(on: bool) {
    ENABLED.store(on, Ordering::Relaxed);
}

pub fn enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

pub fn clear() {
    EVENTS.with(|events| events.borrow_mut().clear());
}

pub fn take() -> Vec<TraceEvent> {
    EVENTS.with(|events| std::mem::take(&mut *events.borrow_mut()))
}

pub fn record(pattern_idx: usize, token_idx: usize, token: &str, cause: String) {
    if !enabled() {
        return;
    }
    EVENTS.with(|events| {
        events.borrow_mut().push(TraceEvent {
            pattern_idx,
            token_idx,
            token: token.to_string(),
            cause,
        })
    });
}
