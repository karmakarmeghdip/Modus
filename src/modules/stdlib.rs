//! Embedded standard library modules for Modus.

pub const STD_IO_SOURCE: &str = include_str!("../../stdlib/io.mds");
pub const STD_FS_SOURCE: &str = include_str!("../../stdlib/fs.mds");

/// Checks if an import specifier is a standard library module (e.g. "std:io", "std:fs").
pub fn is_std_module(specifier: &str) -> bool {
    matches!(specifier, "std:io" | "std:fs")
}

/// Checks if a path represents a standard library module.
pub fn is_std_module_path(path: &std::path::Path) -> bool {
    path.to_str().map(is_std_module).unwrap_or(false)
}

/// Returns the embedded source code for a standard library module path or specifier.
pub fn get_std_module_source(path: &std::path::Path) -> Option<&'static str> {
    match path.to_str() {
        Some("std:io") => Some(STD_IO_SOURCE),
        Some("std:fs") => Some(STD_FS_SOURCE),
        _ => None,
    }
}
