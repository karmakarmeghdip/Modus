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

## Gotchas

- Pratt/precedence climbing for `check`/`perform`/`!`/`-` prefix + postfix call/field/index + binary ops. Test `Result(T, E)` parses as generics, not comparisons.
- Pure functional: all variables immutable (`mut` is a syntax error); no imperative loops (`while`/`for` are syntax errors; use tail recursion).
- Pure by default: `perform` in non-`IO` function = type error; pure function returning `void` = type error (dead computation; functions performing side effects must return `IO(void)` or `IO(T)`); no exceptions; no `dyn`; no GC/RTS; non-atomic RC; heap objects `[u64 rc | payload]`, primitives unboxed.
- Never emit `<>`, `fn`, unparenthesized `if`, `mut`, `while`, `for`, `struct`, `enum`, `Name { ... }`, pure `void` returns, exceptions, GC roots, or atomic RC in code, tests, or fixtures.
