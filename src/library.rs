//! Wrapper libraries: `.resh` files that `IMPORT name` loads. A library is looked up
//! in the user's directory first (so users can add and override wrappers), then
//! among the ones bundled into the binary.

use std::path::{Path, PathBuf};

use crate::BUNDLED;

/// Where a library came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Origin {
    User,
    Bundled,
}

/// The user's library directory: `$RESH_LIB_DIR`, else `$XDG_CONFIG_HOME/resh/lib`,
/// else `~/.config/resh/lib`.
pub fn user_dir() -> Option<PathBuf> {
    let var = |k: &str| std::env::var_os(k).filter(|v| !v.is_empty());
    if let Some(d) = var("RESH_LIB_DIR") {
        return Some(PathBuf::from(d));
    }
    if let Some(d) = var("XDG_CONFIG_HOME") {
        return Some(PathBuf::from(d).join("resh/lib"));
    }
    var("HOME").map(|h| PathBuf::from(h).join(".config/resh/lib"))
}

/// The REPL's init file: `$RESH_INIT`, else `$XDG_CONFIG_HOME/resh/init.resh`,
/// else `~/.config/resh/init.resh`.
pub fn init_path() -> Option<PathBuf> {
    let var = |k: &str| std::env::var_os(k).filter(|v| !v.is_empty());
    if let Some(p) = var("RESH_INIT") {
        return Some(PathBuf::from(p));
    }
    if let Some(d) = var("XDG_CONFIG_HOME") {
        return Some(PathBuf::from(d).join("resh/init.resh"));
    }
    var("HOME").map(|h| PathBuf::from(h).join(".config/resh/init.resh"))
}

/// A library name is one lowercase word (also keeps file lookups the same on
/// case-insensitive file systems), so it can never name a path.
fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-')
}

/// The source of library `name`, from `dir` (the user directory) or the bundle.
pub fn find(name: &str, dir: Option<&Path>) -> Option<(Origin, String)> {
    if !valid_name(name) {
        return None;
    }
    if let Some(dir) = dir
        && let Ok(src) = std::fs::read_to_string(dir.join(format!("{name}.resh")))
    {
        return Some((Origin::User, src));
    }
    BUNDLED
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, src)| (Origin::Bundled, src.to_string()))
}

/// Every importable library, sorted by name; a user library hides a bundled one.
pub fn available(dir: Option<&Path>) -> Vec<(String, Origin)> {
    let mut all: Vec<(String, Origin)> = BUNDLED
        .iter()
        .map(|(n, _)| (n.to_string(), Origin::Bundled))
        .collect();
    if let Some(entries) = dir.and_then(|d| std::fs::read_dir(d).ok()) {
        for e in entries.flatten() {
            let path = e.path();
            let name = path.file_stem().and_then(|s| s.to_str());
            if path.extension().is_some_and(|x| x == "resh")
                && let Some(n) = name.filter(|n| valid_name(n))
            {
                all.retain(|(m, _)| m != n);
                all.push((n.to_string(), Origin::User));
            }
        }
    }
    all.sort();
    all
}
