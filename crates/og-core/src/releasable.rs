use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Instant;

struct CellState<T> {
    value: Option<Arc<T>>,
    last_use: Option<Instant>,
}

pub struct Releasable<T> {
    state: Mutex<CellState<T>>,
    building: Mutex<()>,
}

impl<T> Releasable<T> {
    pub const fn new() -> Self {
        Self {
            state: Mutex::new(CellState {
                value: None,
                last_use: None,
            }),
            building: Mutex::new(()),
        }
    }
}

impl<T> Default for Releasable<T> {
    fn default() -> Self {
        Self::new()
    }
}

type ReleaseFn = std::sync::Arc<dyn Fn(Option<Instant>) + Send + Sync>;

fn registry() -> &'static Mutex<HashMap<usize, ReleaseFn>> {
    static REGISTRY: OnceLock<Mutex<HashMap<usize, ReleaseFn>>> = OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(HashMap::new()))
}

pub fn shared_get_or_build<T>(
    cell: &'static Releasable<T>,
    build: impl FnOnce() -> Option<Arc<T>>,
) -> Option<Arc<T>>
where
    T: Send + Sync + 'static,
{
    let now = Instant::now();
    if let Ok(mut guard) = cell.state.lock()
        && guard.value.is_some()
    {
        let hit = guard.value.clone();
        guard.last_use = Some(now);
        return hit;
    }
    let _build_guard = cell
        .building
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Ok(mut guard) = cell.state.lock()
        && guard.value.is_some()
    {
        let hit = guard.value.clone();
        guard.last_use = Some(now);
        return hit;
    }
    let built = build();
    let mut stored = false;
    let outcome = {
        let Ok(mut guard) = cell.state.lock() else {
            return built;
        };
        if guard.value.is_none() {
            guard.value = built.clone();
            guard.last_use = Some(now);
            stored = true;
        }
        guard.value.clone()
    };
    if stored {
        let key = cell as *const Releasable<T> as usize;
        if let Ok(mut registry) = registry().lock() {
            registry.entry(key).or_insert_with(|| {
                std::sync::Arc::new(move |keep_used_since| release_cell(cell, keep_used_since))
            });
        }
    }
    outcome
}

fn release_cell<T>(cell: &Releasable<T>, keep_used_since: Option<Instant>) {
    let Ok(mut guard) = cell.state.lock() else {
        return;
    };
    if let Some(since) = keep_used_since
        && guard.last_use.is_some_and(|used| used > since)
    {
        return;
    }
    guard.value = None;
}

pub fn release_all() {
    release_with_filter(None);
}

pub fn release_unused_since(since: Instant) {
    release_with_filter(Some(since));
}

fn release_with_filter(keep_used_since: Option<Instant>) {
    let releases: Vec<ReleaseFn> = match registry().lock() {
        Ok(registry) => registry.values().cloned().collect(),
        Err(_) => return,
    };
    for release in releases {
        release(keep_used_since);
    }
}

thread_local! {
    static HEAVY_DEPTH: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

pub struct HeavyGuard {
    locked: Option<std::sync::MutexGuard<'static, ()>>,
}

impl Drop for HeavyGuard {
    fn drop(&mut self) {
        HEAVY_DEPTH.with(|depth| depth.set(depth.get().saturating_sub(1)));
        self.locked.take();
    }
}

// Reentrant on purpose: heavyweight tests hold this across engine builds on the same thread.
pub fn heavy_guard() -> HeavyGuard {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    let locked = if HEAVY_DEPTH.with(|depth| depth.get()) == 0 {
        let lock = LOCK.get_or_init(|| Mutex::new(()));
        Some(lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner()))
    } else {
        None
    };
    HEAVY_DEPTH.with(|depth| depth.set(depth.get() + 1));
    HeavyGuard { locked }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    static SERIALIZE: Mutex<()> = Mutex::new(());

    #[test]
    fn builds_once_and_serves_from_cache() {
        let _serialized = SERIALIZE.lock().unwrap();
        static CELL: Releasable<String> = Releasable::new();
        static BUILDS: AtomicUsize = AtomicUsize::new(0);
        let first = shared_get_or_build(&CELL, || {
            BUILDS.fetch_add(1, Ordering::SeqCst);
            Some(Arc::new("value".to_string()))
        })
        .unwrap();
        let second = shared_get_or_build(&CELL, || {
            BUILDS.fetch_add(1, Ordering::SeqCst);
            Some(Arc::new("other".to_string()))
        })
        .unwrap();
        assert_eq!(BUILDS.load(Ordering::SeqCst), 1);
        assert!(Arc::ptr_eq(&first, &second));
    }

    #[test]
    fn failed_builds_are_not_cached() {
        let _serialized = SERIALIZE.lock().unwrap();
        static CELL: Releasable<String> = Releasable::new();
        static BUILDS: AtomicUsize = AtomicUsize::new(0);
        let build = || {
            BUILDS.fetch_add(1, Ordering::SeqCst);
            None
        };
        assert!(shared_get_or_build(&CELL, build).is_none());
        assert!(shared_get_or_build(&CELL, build).is_none());
        assert_eq!(BUILDS.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn release_all_drops_cached_values_but_keeps_live_arcs() {
        let _serialized = SERIALIZE.lock().unwrap();
        static CELL: Releasable<String> = Releasable::new();
        static BUILDS: AtomicUsize = AtomicUsize::new(0);
        let held = shared_get_or_build(&CELL, || {
            BUILDS.fetch_add(1, Ordering::SeqCst);
            Some(Arc::new("first".to_string()))
        })
        .unwrap();
        release_all();
        assert!(CELL.state.lock().unwrap().value.is_none());
        let rebuilt = shared_get_or_build(&CELL, || {
            BUILDS.fetch_add(1, Ordering::SeqCst);
            Some(Arc::new("second".to_string()))
        })
        .unwrap();
        assert_eq!(BUILDS.load(Ordering::SeqCst), 2);
        assert!(!Arc::ptr_eq(&held, &rebuilt));
        assert_eq!(*held, "first");
        assert_eq!(*rebuilt, "second");
    }

    #[test]
    fn release_unused_since_keeps_cells_used_after_the_instant() {
        let _serialized = SERIALIZE.lock().unwrap();
        static USED: Releasable<String> = Releasable::new();
        static IDLE: Releasable<String> = Releasable::new();
        static USED_BUILDS: AtomicUsize = AtomicUsize::new(0);
        static IDLE_BUILDS: AtomicUsize = AtomicUsize::new(0);
        let _idle = shared_get_or_build(&IDLE, || {
            IDLE_BUILDS.fetch_add(1, Ordering::SeqCst);
            Some(Arc::new("idle".to_string()))
        })
        .unwrap();
        let since = Instant::now();
        std::thread::sleep(Duration::from_millis(2));
        let used = shared_get_or_build(&USED, || {
            USED_BUILDS.fetch_add(1, Ordering::SeqCst);
            Some(Arc::new("used".to_string()))
        })
        .unwrap();
        release_unused_since(since);
        assert!(
            USED.state.lock().unwrap().value.is_some(),
            "a cell used after the instant must survive"
        );
        assert!(
            IDLE.state.lock().unwrap().value.is_none(),
            "a cell idle since before the instant must be released"
        );
        let again = shared_get_or_build(&USED, || {
            USED_BUILDS.fetch_add(1, Ordering::SeqCst);
            Some(Arc::new("rebuilt".to_string()))
        })
        .unwrap();
        assert!(Arc::ptr_eq(&used, &again));
        assert_eq!(USED_BUILDS.load(Ordering::SeqCst), 1);
        assert_eq!(IDLE_BUILDS.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn concurrent_callers_build_exactly_once() {
        let _serialized = SERIALIZE.lock().unwrap();
        static CELL: Releasable<Vec<usize>> = Releasable::new();
        static BUILDS: AtomicUsize = AtomicUsize::new(0);
        let results: Mutex<Vec<Arc<Vec<usize>>>> = Mutex::new(Vec::new());
        std::thread::scope(|scope| {
            for t in 0..4 {
                let results = &results;
                scope.spawn(move || {
                    let value = shared_get_or_build(&CELL, || {
                        BUILDS.fetch_add(1, Ordering::SeqCst);
                        Some(Arc::new(vec![t]))
                    })
                    .unwrap();
                    results.lock().unwrap().push(value);
                });
            }
        });
        let results = results.into_inner().unwrap();
        assert_eq!(BUILDS.load(Ordering::SeqCst), 1);
        assert_eq!(results.len(), 4);
        assert!(results.iter().all(|r| Arc::ptr_eq(r, &results[0])));
    }

    #[test]
    fn heavy_guard_is_reentrant_on_the_same_thread() {
        let outer = heavy_guard();
        let inner = heavy_guard();
        drop(inner);
        drop(outer);
        let fresh = heavy_guard();
        drop(fresh);
    }

    #[test]
    fn heavy_guard_blocks_other_threads_until_released() {
        let outer = heavy_guard();
        let (tx, rx) = std::sync::mpsc::channel();
        let handle = std::thread::spawn(move || {
            let _guard = heavy_guard();
            tx.send(()).unwrap();
        });
        assert!(
            rx.recv_timeout(Duration::from_millis(300)).is_err(),
            "a second thread must wait while the guard is held"
        );
        drop(outer);
        rx.recv_timeout(Duration::from_secs(5))
            .expect("second thread must acquire the guard after release");
        handle.join().unwrap();
    }

    #[test]
    fn poisoned_cell_lock_returns_the_build_without_caching() {
        let _serialized = SERIALIZE.lock().unwrap();
        static CELL: Releasable<String> = Releasable::new();
        static BUILDS: AtomicUsize = AtomicUsize::new(0);
        let poisoned = std::thread::spawn(move || {
            let _guard = CELL.state.lock().unwrap();
            panic!("poison the cell lock");
        })
        .join();
        assert!(poisoned.is_err());
        let first = shared_get_or_build(&CELL, || {
            BUILDS.fetch_add(1, Ordering::SeqCst);
            Some(Arc::new("a".to_string()))
        });
        let second = shared_get_or_build(&CELL, || {
            BUILDS.fetch_add(1, Ordering::SeqCst);
            Some(Arc::new("b".to_string()))
        });
        assert_eq!(first.as_deref().map(String::as_str), Some("a"));
        assert_eq!(second.as_deref().map(String::as_str), Some("b"));
        assert_eq!(BUILDS.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn poisoned_build_lock_recovers_and_caches() {
        let _serialized = SERIALIZE.lock().unwrap();
        static CELL: Releasable<String> = Releasable::new();
        static BUILDS: AtomicUsize = AtomicUsize::new(0);
        let poisoned = std::thread::spawn(move || {
            let _guard = CELL.building.lock().unwrap();
            panic!("poison the build lock");
        })
        .join();
        assert!(poisoned.is_err());
        let first = shared_get_or_build(&CELL, || {
            BUILDS.fetch_add(1, Ordering::SeqCst);
            Some(Arc::new("a".to_string()))
        })
        .unwrap();
        let second = shared_get_or_build(&CELL, || {
            BUILDS.fetch_add(1, Ordering::SeqCst);
            Some(Arc::new("b".to_string()))
        })
        .unwrap();
        assert_eq!(BUILDS.load(Ordering::SeqCst), 1);
        assert!(Arc::ptr_eq(&first, &second));
    }
}
