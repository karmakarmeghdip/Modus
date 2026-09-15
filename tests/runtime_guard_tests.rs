//! P0.1 — Runtime escape-hatch guard.
//!
//! `src/backend/runtime.rs` must only register the building blocks listed in
//! `AGENTS.md` (Minimal Runtime). This test scans the runtime source for every
//! LLVM function it registers (`add_function` / `get_function`) and asserts the
//! set is *exactly* `ALLOWED` ∪ `LEGACY`.
//!
//! - A symbol outside both sets fails the test: new escape hatches are blocked
//!   while the string migration (P1) is in progress.
//! - A `LEGACY` entry that no longer exists fails the test: the legacy list
//!   must be shrunk as each P1 step deletes a symbol, so the list can never
//!   drift out of sync with the runtime.
//!
//! When P1.8 completes, `LEGACY` is empty and the guard asserts the bare
//! allow-list.

use std::collections::BTreeSet;
use std::path::Path;

/// Building blocks the runtime is allowed to register at all times.
const ALLOWED: &[&str] = &[
    // Core memory + reference counting
    "malloc",
    "free",
    "memcpy",
    "modus_alloc",
    "modus_inc_ref",
    "modus_dec_ref",
    "modus_is_unique",
    // [T] array-buffer primitives (FBIP)
    "modus_array_new",
    "modus_array_push",
    "modus_array_build",
    "modus_array_set",
    "modus_array_pop",
];

/// Symbols still present during the P1 migration. Each entry names the P1 step
/// that deletes it. Remove an entry from this list in the same change that
/// removes the symbol from `runtime.rs`.
///
/// - P1.1 (String `==/!=` via `Eq` impl): DONE.
/// - P1.2 (String `+` via `Add` impl): DONE — `modus_str_concat` deleted.
/// - P1.3 (String substring/slice via stdlib): `modus_str_substring`
/// - P1.5 (fromCStr via FFI, fromCharCode via stdlib): `modus_string_from_c_str`,
///   `modus_str_from_char_code`, `strlen`
/// - P1.6 (Show via stdlib): `snprintf`
const LEGACY: &[&str] = &[
    "modus_str_substring",
    "modus_string_from_c_str",
    "modus_str_from_char_code",
    "strlen",
    "snprintf",
];

/// Collects every function name registered via `add_function("X")` or
/// `get_function("X")` in the runtime source.
fn registered_symbols(source: &str) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    for marker in ["add_function(\"", "get_function(\""] {
        let mut search_from = 0;
        while let Some(rel) = source[search_from..].find(marker).map(|p| p + search_from) {
            let start = rel + marker.len();
            let end = source[start..]
                .find('"')
                .expect("unterminated function name in runtime.rs");
            names.insert(source[start..start + end].to_string());
            search_from = start + end;
        }
    }
    names
}

#[test]
fn test_runtime_only_registers_allowed_symbols() {
    let runtime_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .join("backend")
        .join("runtime.rs");
    let source = std::fs::read_to_string(&runtime_path)
        .unwrap_or_else(|e| panic!("Failed to read {:?}: {e}", runtime_path));

    let registered = registered_symbols(&source);

    let allowed: BTreeSet<&str> = ALLOWED.iter().copied().collect();
    let legacy: BTreeSet<&str> = LEGACY.iter().copied().collect();
    let mut expected: BTreeSet<&str> = allowed.clone();
    expected.extend(legacy.iter().copied());

    let unexpected: Vec<&String> = registered
        .iter()
        .filter(|name| !expected.contains(name.as_str()))
        .collect();
    let missing_legacy: Vec<&str> = legacy
        .iter()
        .copied()
        .filter(|name| !registered.contains(*name))
        .collect();

    assert!(
        unexpected.is_empty(),
        "runtime.rs registers symbols outside the allow-list (new escape hatches \
         are forbidden; move the functionality to stdlib instead):\n  {:?}",
        unexpected
    );
    assert!(
        missing_legacy.is_empty(),
        "LEGACY list in this test is stale — these symbols no longer exist in \
         runtime.rs, remove them from LEGACY:\n  {missing_legacy:?}"
    );
}

#[test]
fn test_legacy_list_only_contains_forbidden_patterns() {
    // Every legacy entry must be a symbol the refactor plan explicitly forbids
    // in runtime.rs: a `modus_str_*`/`modus_string_*` helper or a C declaration
    // that exists solely to serve one.
    const LEGACY_C_DECLS: &[&str] = &["strlen", "snprintf", "memcmp"];
    for name in LEGACY {
        assert!(
            name.starts_with("modus_str_")
                || name.starts_with("modus_string_")
                || name.starts_with("modus_show_")
                || name.starts_with("modus_bignum_")
                || LEGACY_C_DECLS.contains(name),
            "LEGACY entry '{name}' is not a forbidden pattern; it belongs in \
             ALLOWED or must be removed"
        );
    }
    // Allowed symbols must never be listed as legacy.
    for name in ALLOWED {
        assert!(
            !LEGACY.contains(name),
            "symbol '{name}' is both ALLOWED and LEGACY"
        );
    }
}
