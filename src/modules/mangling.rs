//! Symbol mangling utilities for cross-module function calls and dynamic libraries.

use std::path::Path;

/// Sanitizes a string to contain only valid C/LLVM identifier characters (`a-z`, `A-Z`, `0-9`, `_`).
pub fn sanitize_ident(s: &str) -> String {
    let mut result = String::new();
    for c in s.chars() {
        if c.is_ascii_alphanumeric() || c == '_' {
            result.push(c);
        } else {
            result.push('_');
        }
    }
    if result.is_empty() {
        "mod".to_string()
    } else {
        result
    }
}

/// Derives a canonical module identifier from a file or library path.
/// E.g.:
/// - `"math.mds"` -> `"math"`
/// - `"libmath.so"` -> `"math"`
/// - `"matrix_ops.mds"` -> `"matrix_ops"`
pub fn module_ident_from_path(path: &Path) -> String {
    if let Some(s) = path.to_str()
        && let Some(stripped) = s.strip_prefix("std:")
    {
        return format!("std_{}", sanitize_ident(stripped));
    }
    let file_stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("module");
    let clean_stem = if let Some(stripped) = file_stem.strip_prefix("lib") {
        if !stripped.is_empty() {
            stripped
        } else {
            file_stem
        }
    } else {
        file_stem
    };
    sanitize_ident(clean_stem)
}

/// Mangles an exported function symbol for cross-module linking according to the Modus ABI:
/// `_modus_M_{module_ident}_{function_name}`
/// Exception: `main` is always emitted as `main` to serve as the runtime entry point.
pub fn mangle_symbol(module_ident: &str, fn_name: &str) -> String {
    if fn_name == "main" {
        "main".to_string()
    } else {
        format!("_modus_M_{module_ident}_{fn_name}")
    }
}

/// Returns true if the symbol name has been mangled according to Modus ABI.
pub fn is_mangled_symbol(name: &str) -> bool {
    name.starts_with("_modus_M_")
}
