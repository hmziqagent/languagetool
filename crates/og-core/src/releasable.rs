use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

pub struct Releasable<T> {
    cell: Mutex<Option<Arc<T>>>,
}

impl<T> Releasable<T> {
    pub const fn new() -> Self {
        Self {
            cell: Mutex::new(None),
        }
    }
}

impl<T> Default for Releasable<T> {
    fn default() -> Self {
        Self::new()
    }
}

fn registry() -> &'static Mutex<HashMap<usize, Box<dyn Fn() + Send + Sync>>> {
    static REGISTRY: OnceLock<Mutex<HashMap<usize, Box<dyn Fn() + Send + Sync>>>> = OnceLock::new();
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
    let built = build();
    if let Ok(mut guard) = cell.cell.lock() {
        if guard.is_none() {
            *guard = built.clone();
            let key = cell as *const Releasable<T> as usize;
            if let Ok(mut registry) = registry().lock() {
                registry.entry(key).or_insert_with(|| {
                    Box::new(move || {
                        if let Ok(mut guard) = cell.cell.lock() {
                            *guard = None;
                        }
                    })
                });
            }
        }
        return guard.clone();
    }
    built
}

pub fn release_all() {
    if let Ok(registry) = registry().lock() {
        for release in registry.values() {
            release();
        }
    }
}

pub struct HeavyGuard(std::sync::MutexGuard<'static, ()>);

pub fn heavy_guard() -> HeavyGuard {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    let lock = LOCK.get_or_init(|| Mutex::new(()));
    HeavyGuard(lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner()))
}
