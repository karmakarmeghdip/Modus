# AGENTS.md

Modus compiler (Rust). Greenfield: `src/main.rs` is hello-world, `Cargo.toml` has no deps. Spec: `docs/SPEC.md`.

## Commands

- `devenv shell`; `cargo build`, `cargo test`, `cargo run`
- Focused: `cargo test <name> -- --nocapture`
- Before done: `cargo fmt --check && cargo clippy -- -D warnings && cargo test`
- `inkwell` needs matching system LLVM; prefer `chumsky` parser/AST work first if LLVM is unavailable.

## Syntax (canonical — see SPEC for details)

- `function`, never `fn`: `function f(T: Drawable)(x: T): T`
- `if (cond)` / `else if (cond)` — parens always required.
- Generics parenthesized, never `<>`: `Result(T, E)`.
- Variables are strictly immutable: `let x: T = expr;` — no `mut` keyword.
- No loops (`while`, `for`): purely functional iteration via tail recursion (`musttail`), closures, and pattern matching.
- Data types use `type` (never `struct` or `enum`): records `type Point = { x: i32, y: i32 }`, discriminated unions `type Option(T) = Some(T) | None`.
- Record instantiation uses `{ field: value, ... }` directly without a type name prefix (never `Name { ... }`); typing is determined from the `let` binding, param, or return type: `let p: Point = { x: 1, y: 2 };`.

## Pipeline (build in this order)

`chumsky` parse → typed AST → inference + traits + purity → desugar → ANF + closures → liveness/borrow → Perceus + FBIP → `inkwell` LLVM `-O3`. Planned dirs: `src/ast/`, typechecker, ANF/IR, backend.

## Compiler Minimalism & Runtime Discipline

- **Minimal Compiler**: The compiler should just compile and nothing else. Keep the compiler as lean and minimal as possible; implement as much functionality as possible in Modus itself.
- **No `runtime.rs` Escape Hatches**: Never extend `runtime.rs` with one-off helper functions or runtime shims to circumvent or escape-hatch language limitations. The compiler runtime is strictly reserved for foundational primitives: memory allocation (`modus_alloc`, `modus_free`), Perceus refcount headers, and panic/abort traps.
- **Self-Contained Standard Library**: Any functionality that can be implemented in Modus must be implemented in Modus. Keep the stdlib as independent of libc as possible; libc is strictly reserved for non-bypassable OS interactions (descriptors, syscalls, clocks).
- **Mandatory `IO` on `extern "C"`**: All `extern "C"` functions must always return `IO(...)` (`IO(T)` or `IO(void)`). Pure functions can and must be implemented in Modus itself, never escape-hatched from external C libraries. This guarantees language soundness and ensures foreign code can never bypass the pure type system.
- **Calling Conventions**: All Modus-to-Modus functions (internal functions, lifted closures/lambdas, and inter-module functions) must use `fastcc` (8). Standard C convention (`ccc` = 0) is strictly reserved for `main` (OS entrypoint), `extern "C"` FFI, and functions exported from the root entrypoint file when compiled with `--lib`.

## Gotchas

- Pratt/precedence climbing for `check`/`perform`/`!`/`-` prefix + postfix call/field/index + binary ops. Test `Result(T, E)` parses as generics, not comparisons.
- Pure functional: all variables immutable (`mut` is a syntax error); no imperative loops (`while`/`for` are syntax errors; use tail recursion).
- Pure by default: `perform` in non-`IO` function = type error; pure function returning `void` = type error (dead computation; functions performing side effects must return `IO(void)` or `IO(T)`); no exceptions; no `dyn`; no GC/RTS; non-atomic RC; heap objects `[u64 rc | payload]`, primitives unboxed.
- Never emit `<>`, `fn`, unparenthesized `if`, `mut`, `while`, `for`, `struct`, `enum`, `Name { ... }`, pure `void` returns, exceptions, GC roots, or atomic RC in code, tests, or fixtures.
- Never declare an `extern "C"` function that returns a pure non-`IO` type. All foreign functions must return `IO(...)`.
