pub mod en;
pub mod engine;
pub mod registry;

pub use en::*;
pub use engine::*;
pub use registry::*;

pub(crate) fn release_guard() -> impl Drop {
    struct ReleaseOnDrop(og_core::releasable::HeavyGuard);
    impl Drop for ReleaseOnDrop {
        fn drop(&mut self) {
            og_core::releasable::release_all();
            #[cfg(target_os = "linux")]
            unsafe {
                libc::malloc_trim(0);
            }
        }
    }
    ReleaseOnDrop(og_core::releasable::heavy_guard())
}

pub(crate) fn weak_shared<T>(
    build: impl FnOnce() -> Option<std::sync::Arc<T>>,
) -> Option<std::sync::Arc<T>>
where
    T: Send + Sync + 'static,
{
    use std::any::{Any, TypeId};
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex, OnceLock, Weak};
    static CACHE: OnceLock<Mutex<HashMap<TypeId, Weak<dyn Any + Send + Sync>>>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let key = TypeId::of::<T>();
    if let Some(hit) = cache
        .lock()
        .ok()
        .and_then(|guard| guard.get(&key).cloned())
        .and_then(|weak| weak.upgrade())
        .and_then(|any| any.downcast::<T>().ok())
    {
        return Some(hit);
    }
    let built = build()?;
    if let Ok(mut guard) = cache.lock() {
        let erased: Arc<dyn Any + Send + Sync> = built.clone();
        guard.insert(key, Arc::downgrade(&erased));
    }
    Some(built)
}
pub mod ar;
pub mod ast;
pub mod be;
pub mod br;
pub mod ca;
pub mod confusion_probability;
pub mod crh;
pub mod da;
pub mod de;
pub mod detection;
pub mod el;
pub mod eo;
pub mod es;
pub mod fa;
pub mod fr;
pub mod ga;
pub mod gl;
pub mod is;
pub mod it;
pub mod ja;
pub mod km;
pub mod lt;
pub mod match_priority;
pub mod ml;
pub mod nl;
pub mod paragraph_repeat_beginning;
pub mod pl;
pub mod pt;
pub mod ro;
pub mod ru;
pub mod sk;
pub mod sl;
pub mod specific_case;
pub mod sr;
pub mod sv;
pub mod ta;
pub mod tl;
pub mod uk;
pub mod upper_case_ngram;
pub mod zh;
