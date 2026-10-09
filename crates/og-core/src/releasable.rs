use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

pub struct Releasable<T> {
    cell: Mutex<Option<Arc<T>>>,
    building: Mutex<()>,
}

impl<T> Releasable<T> {
    pub const fn new() -> Self {
        Self {
            cell: Mutex::new(None),
            building: Mutex::new(()),
        }
    }
}

impl<T> Default for Releasable<T> {
    fn default() -> Self {
        Self::new()
    }
}

fn registry() -> &'static Mutex<HashMap<usize, std::sync::Arc<dyn Fn() + Send + Sync>>> {
    static REGISTRY: OnceLock<Mutex<HashMap<usize, std::sync::Arc<dyn Fn() + Send + Sync>>>> =
        OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(HashMap::new()))
}

pub fn shared_get_or_build<T>(
    cell: &'static Releasable<T>,
    build: impl FnOnce() -> Option<Arc<T>>,
) -> Option<Arc<T>>
where
    T: Send + Sync + 'static,
{
    if let Ok(guard) = cell.cell.lock()
        && let Some(hit) = guard.as_ref()
    {
        return Some(hit.clone());
    }
    let _build_guard = cell
        .building
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Ok(guard) = cell.cell.lock()
        && let Some(hit) = guard.as_ref()
    {
        return Some(hit.clone());
    }
    let built = build();
    let mut stored = false;
    let outcome = {
        let Ok(mut guard) = cell.cell.lock() else {
            return built;
        };
        if guard.is_none() {
            *guard = built.clone();
            stored = true;
        }
        guard.clone()
    };
    if stored {
        let key = cell as *const Releasable<T> as usize;
        if let Ok(mut registry) = registry().lock() {
            registry.entry(key).or_insert_with(|| {
                std::sync::Arc::new(move || {
                    if let Ok(mut guard) = cell.cell.lock() {
                        *guard = None;
                    }
                })
            });
        }
    }
    outcome
}

pub fn release_all() {
    let releases: Vec<std::sync::Arc<dyn Fn() + Send + Sync>> = match registry().lock() {
        Ok(registry) => registry.values().cloned().collect(),
        Err(_) => return,
    };
    for release in releases {
        release();
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
