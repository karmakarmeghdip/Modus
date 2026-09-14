# Runtime Minimization Refactor TODO

Goal: `src/backend/runtime.rs` contains **only the building blocks that cannot be
written in pure Modus**. Everything else (`String`, bignums, maps/sets, …) is
built in Modus on top of those blocks.

Target runtime (see `AGENTS.md` — Minimal Runtime):

- Primitives: unboxed `u8|u16|u32|u64|i8|i16|i32|i64|f32|f64|bool|void` layouts only.
- `Record`: fixed-size heap struct `[u64 rc | payload]`, RC-tracked, codegen-owned layout/GEP + FBIP update.
- `[T]`: the single growable/shrinkable buffer `[rc | len | cap | elems]` + FBIP buffer ops only (`new/push/build/set/pop/len/cap`-style).
- Core: `modus_alloc`, `malloc`/`free` decls (+ `memcpy`/`memmove` decls only as codegen intrinsics for record/array copies), `modus_inc_ref`, `modus_dec_ref` (with `drop_fields` + `free`), `modus_is_unique`.
- Traps: panic/abort/bounds-check handlers.

Explicitly forbidden in `runtime.rs`: `modus_str_*`, `modus_string_*`, `Show`,
`toCString/fromCString`, bignum, map/set, domain/OS logic.

## Current state (inventory)

`src/backend/runtime.rs` currently emits (beyond the allowed core):

- `modus_str_concat` (`build_str_concat_fn:389`), `modus_str_eq` (`:629`),
  `modus_str_substring` (`:770`), `modus_string_from_c_str` (`:971`),
  `modus_str_from_char_code` (`:1100`).
- C decls used only by the above: `strlen`, `memcmp`, `snprintf` (`:61-77,99-103`).
  `memcpy` is shared — keep only for record/`[T]` copies.
- `Runtime` struct fields `str_*`, `string_from_c_str`, `array_*` (`:30-39`).
  `array_*` (`new/push/build/set/pop`) **stay** — they are the `[T]` building block.

Codegen / typechecker call sites that must be decoupled:

- `src/backend/codegen/expr.rs:55` `String ==/!=` → `str_eq_fn`.
- `src/backend/codegen/expr.rs:249-310,487` `String.toCString` / `fromCStr` / `fromCharCode`.
- `src/backend/codegen/expr.rs:810,929-962` `charCodeAt` / `substring` method intrinsics.
- `src/backend/codegen/ops.rs:241-243` `String +` → `str_concat_fn`.
- `src/backend/codegen/ops.rs:84` string-literal globals (`modus_str_lit`).
- `src/typechecker/infer/synth.rs:211` `String.toCString`, `:513,561,575` `.length()`,
  `:588` `.charCodeAt`, `:611` `.substring` special-casing.
- Tests asserting runtime symbols: `tests/codegen_tests.rs:140-141,162`
  (`modus_alloc` OK to keep; any `modus_str_*` assertions must go).

`stdlib/string.mds` already implements `split`, `toLowerCase/UpperCase`,
`fromCharCode`, `fround`-style logic in pure Modus — follow that pattern for the
remaining ops. `docs/STDLIB.md:30-38,318-324` still documents `str_concat/str_eq`
as runtime-owned and needs updating at the end.

## Decision needed before code (P0)

- [ ] D1 — `String` representation in pure Modus. Options:
  - A (recommended): keep `String` as a surface type but lower it in codegen to the
    same heap-buffer layout as `[u8]` (`[rc | len | cap | bytes + NUL]`), so all
    `String` ops become `stdlib/string.mds` functions over `[u8]` buffer ops +
    `Pointer(u8)` + `extern "C" IO(...)` for OS/C interop. NUL-termination kept
    only at the FFI boundary (`toCString` = pointer-offset view, no alloc).
  - B: define `type String = { buf: [u8], len: i64 }` (or similar record) fully in
    stdlib. More records/RC traffic; only pick if A blocks FFI layout.
- [ ] D2 — literal story: keep string literals as codegen-emitted read-only
  `[u8]` buffers (immortal `rc <= 0` path in `inc/dec_ref` already handles this),
  or emit them as static bytes + a stdlib constructor call. Record choice in this file.

## Refactor steps (in order)

### P0 — Freeze the contract, stop the bleed

- [ ] P0.1 Add a guard test: fail if `runtime.rs` defines/registers any symbol
  matching `modus_str_*`, `modus_string_*`, `modus_show_*`, `modus_bignum_*`,
  or any new `add_function` outside the allow-list
  (`modus_alloc/inc_ref/dec_ref/is_unique` + `array_*` + traps + `malloc/free/memcpy`
  decls). Prevents new escape hatches while migrating.
- [ ] P0.2 Enforce `AGENTS.md` rule in review: no new `Runtime` struct fields or
  `build_*_fn` outside the allow-list.

### P1 — Rebuild `String` on `[u8]` / `[T]` in stdlib (one op at a time, tests green)

Each item: implement in `stdlib/string.mds` (pure Modus, tail recursion, no loops),
wire typechecker to resolve to stdlib instead of the intrinsic, switch codegen
lowering to a plain call, keep old runtime fn until the new path is tested, then delete.

- [ ] P1.1 `eq`: pure `stringEq(a, b): bool` over `length` + `charCodeAt` loop
  (or `[u8]` compare). Replace `expr.rs:55` `str_eq_fn` call. Delete `build_str_eq_fn` + `memcmp` decl if unused elsewhere.
- [ ] P1.2 `concat` (`+`): pure `concat(a, b): String` over `[u8]` push/append with
  FBIP reuse when unique. Replace `ops.rs:241` `str_concat_fn` call. Delete `build_str_concat_fn`.
- [ ] P1.3 `substring/slice`: pure bounds-clamp + copy over `[u8]`. Replace
  `expr.rs:929-962` + `synth.rs:611`. Delete `build_str_substring_fn`.
- [ ] P1.4 `length/charCodeAt`: expose as array-buffer `len` + indexed `u8` load
  (no per-call runtime helper; codegen emits the same GEP/load it already does for
  `[T]`). Remove `synth.rs:513,561,575,588` special cases once stdlib covers them.
- [ ] P1.5 `fromCharCode/fromCStr/toCString/CString.toString`:
  - `fromCharCode`: single-element `[u8]` push in pure Modus. Delete `build_str_from_char_code_fn`.
  - `fromCStr` / `CString.toString`: implement via `Pointer(u8)` + `extern "C"` `strlen`-style
    read in stdlib (`IO`-wrapped where it touches foreign memory), not a runtime helper.
    Delete `build_string_from_c_str_fn` + `strlen` decl (keep `strlen` only if re-exposed as a normal `extern "C" IO(...)` stdlib decl, not a runtime field).
  - `toCString`: keep as a zero-copy codegen pointer-offset view (`data at +24` / buffer base),
    no `modus_*` helper, no alloc. Remove `synth.rs:211` intrinsic once it resolves to stdlib.
- [ ] P1.6 `Show` for primitives + templating: keep desugar → `+` + `.show()` but make
  `show` resolve to `stdlib` pure functions (int/float formatting via `[u8]` digit loops,
  not `snprintf` in runtime). Delete `snprintf` decl/field.
- [ ] P1.7 Literals: after D2, make string literals construct the D1 layout directly
  (no `modus_str_lit` global that only runtime helpers understand). Update `ops.rs:84`.

### P1 — Cleanup + docs

- [ ] P1.8 Shrink `Runtime` struct to allow-list fields only; delete all `str_*/string_*`
  fields, builders, and doc comments (`runtime.rs:10,30-34,105-131,389-1153`).
- [ ] P1.9 Update `docs/STDLIB.md` (runtime-owns-`str_concat/str_eq` claims, §1.6/§4) and
  `docs/SPEC.md` §4 (`String` is stdlib-on-`[u8]`, not a runtime primitive) + `README` FBIP example if it references `modus_str_*`.
- [ ] P1.10 Update `tests/codegen_tests.rs` (`modus_str_*` assertions → stdlib-call or buffer-op
  assertions; `modus_alloc` assertions stay).

### P2 — Prove the building blocks suffice (follow-ups, not blockers)

- [ ] P2.1 Bignum spike in pure Modus over `[u64]`/`[u8]` + primitives (add/mul/compare),
  no runtime changes — validates the "bignums on top of `[T]`" claim.
- [ ] P2.2 Audit remaining `array_*` helpers against the FBIP spec: keep only true buffer
  primitives; anything expressible via `new/push/set/pop` + codegen moves to stdlib.
- [ ] P2.3 Confirm `Record` needs no new runtime helpers (layout/GEP/FBIP in codegen only);
  `drop_fields` stays part of `dec_ref`, not a separate domain helper.
- [ ] P2.4 `no_std`/bare-metal check: the shrunk runtime (alloc + RC + traps + array/record
  buffers) builds without libc domain deps.

## Acceptance criteria

- `runtime.rs` exposes only the allow-list; P0.1 guard test passes.
- No `modus_str_*` / `modus_string_*` symbols in emitted LLVM for `stdlib` + `examples` builds.
- `String` tests (`tests/stdlib_string_tests.rs`, `show_and_template_tests.rs`) pass via the
  pure-Modus implementations with no runtime string helpers linked.
- `cargo fmt --check && cargo clippy -- -D warnings && cargo test` green.
