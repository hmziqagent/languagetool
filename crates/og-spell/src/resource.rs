//! Configurable LanguageTool resource-root resolution.
//!
//! All access to the upstream Java LanguageTool data files (grammar.xml,
//! disambiguation.xml, dictionaries, rule data files) goes through the root
//! resolved here. The root points at the `languagetool-language-modules`
//! directory of the read-only upstream checkout (`<repo>/languagetool/`).
//!
//! Resolution order:
//! 1. `OG_LT_RESOURCE_ROOT` environment variable (absolute path to the
//!    `languagetool-language-modules` directory).
//! 2. Ancestor walk from the current working directory looking for
//!    `languagetool/languagetool-language-modules`.
//! 3. Ancestor walk from this crate's compile-time manifest directory.
//!
//! No absolute paths are baked in.

use std::path::{Path, PathBuf};

/// Environment variable that overrides LT resource-root discovery.
pub const LT_RESOURCE_ROOT_ENV: &str = "OG_LT_RESOURCE_ROOT";

/// Marker directory layout inside the upstream checkout that identifies the
/// resource root: `<root>/en/src/main/resources/org/languagetool/resource/en`.
const PROBE_SUFFIX: &str = "en/src/main/resources/org/languagetool/resource/en";

/// Max levels to walk up when probing ancestors for the upstream checkout.
const MAX_ANCESTOR_HOPS: usize = 8;

/// Resolve the LanguageTool resource root (the `languagetool-language-modules`
/// directory), if one can be found.
pub fn lt_resource_root() -> Option<PathBuf> {
    // 1. Explicit override.
    if let Ok(root) = std::env::var(LT_RESOURCE_ROOT_ENV) {
        let path = PathBuf::from(root);
        if is_resource_root(&path) {
            return Some(path);
        }
        return None;
    }

    // 2. Walk up from the current working directory.
    if let Ok(cwd) = std::env::current_dir() {
        if let Some(root) = find_in_ancestors(&cwd) {
            return Some(root);
        }
    }

    // 3. Walk up from this crate's manifest directory (compile-time location),
    //    which keeps tests working regardless of the runtime cwd.
    if let Some(manifest) = option_env!("CARGO_MANIFEST_DIR") {
        if let Some(root) = find_in_ancestors(Path::new(manifest)) {
            return Some(root);
        }
    }

    None
}

/// Read a resource file located at `rel` under the LT resource root.
///
/// `rel` is relative to the `languagetool-language-modules` directory, e.g.
/// `"en/src/main/resources/org/languagetool/rules/en/replace.txt"`.
/// Returns `None` when the root is unavailable or the file cannot be read.
pub fn read_lt_resource(rel: &str) -> Option<String> {
    let path = lt_resource_root()?.join(rel);
    std::fs::read_to_string(path).ok()
}

fn is_resource_root(path: &Path) -> bool {
    path.is_dir() && path.join(PROBE_SUFFIX).is_dir()
}

fn find_in_ancestors(start: &Path) -> Option<PathBuf> {
    let mut current = Some(start);
    for _ in 0..MAX_ANCESTOR_HOPS {
        let dir = current?;
        let candidate = dir
            .join("languagetool")
            .join("languagetool-language-modules");
        if is_resource_root(&candidate) {
            return candidate.canonicalize().ok();
        }
        current = dir.parent();
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resource_root_resolves_or_none_without_panic() {
        // Either finds the checkout or returns None; must never panic and,
        // when found, must actually contain English resources.
        if let Some(root) = lt_resource_root() {
            assert!(
                root.join(PROBE_SUFFIX).is_dir(),
                "root missing English resources"
            );
        }
    }

    #[test]
    fn read_lt_resource_english_grammar_when_root_present() {
        if read_lt_resource("en/src/main/resources/org/languagetool/rules/en/grammar.xml").is_some()
        {
            let root = lt_resource_root().expect("root must exist if grammar.xml was read");
            assert!(root.join("en").is_dir());
        }
    }
}
