# Modus Spec

Pure-by-default systems language: TypeScript ergonomics, Haskell purity (`IO`/`perform`), `Result`/`check` errors (no exceptions), Rust primitives, Perceus RC, share-nothing threads. Compiler in Rust (`chumsky` → `inkwell`).

## 1. Surface syntax

- Functions: `function name(T: Bound)(x: T): Ret { ... }` or `=> expr`. Trait members same keyword.
- `if (cond) { ... } else if (cond) { ... } else { ... }` — parens always required.
- Generics parenthesized, never `<>`: `Result(T, E)`.
- Variables: strictly immutable (`let x: T = expr;` or `let x = expr;`). No `mut` keyword.
- Control flow & iteration: no imperative loops (`while`, `for`); iteration is purely functional via tail recursion (`musttail`), closures, and pattern matching.
- Types: `u8|u16|u32|u64|i8|i16|i32|i64|f32|f64|bool|String|void`, `[T]`, `(A, B) => C`, `{ x: T }`.
- Records: `{ x: 1 }`, update `{ ...cfg, port: 8080 }`. Record instantiation is purely structural `{ ... }` without `TypeName { ... }` prefix; typing is inferred from the `let` annotation, parameter type, or return type: `let p: Point = { x: 1, y: 2 };`. Closures: `(x: T) => expr`.
- Effects: `perform expr` unwraps `IO(T)` (caller must return `IO(...)`). Errors: `check expr` early-returns `Err` via `FromResidual`.
- Data types: declared with `type` (never `struct` or `enum`):
  - Records: `type Point = { x: i32, y: i32 };`
  - Discriminated Unions: `type Option(T) = Some(T) | None;`, `type Result(T, E) = Ok(T) | Err(E);`
- Match: `match expr { Pattern => expr, ... }`, pats: `_ | ident | Variant(...) | { f: pat } | (a, b)`.
- Traits: `trait Drawable(Self) { function draw(self: Self): IO(void); }`, `impl X for T { ... }`. No `dyn` — `item: Drawable` is dynamic (fat ptr + vtable), `function f(T: Drawable)(x: T)` is static (monomorphized).

```typescript
type Circle = { radius: f64 };
type Rect = { w: f64, h: f64 };
type Option(T) = Some(T) | None;
type Result(T, E) = Ok(T) | Err(E);

function renderStatic(T: Drawable)(item: T): IO(void) { perform item.draw(); }
function renderDynamic(item: Drawable): IO(void) { perform item.draw(); }

let c: Circle = { radius: 2.0 };
let r: Rect = { w: 4.0, h: 1.0 };
let list: [Drawable] = [c, r];

trait Try(Self) { function branch(self: Self): ControlFlow(Self.Residual, Self.Output); }
trait FromResidual(Self, Residual) { function fromResidual(res: Residual): Self; }
```

Key grammar (full EBNF was trimmed — this is what the parser must get right):

```ebnf
FunctionDecl ::= "function" Identifier TypeParams? "(" ParamList? ")" (":" TypeExpr)? (Block | "=>" Expr)
TraitMember  ::= "function" Identifier "(" ParamList? ")" ":" TypeExpr ";"
TypeDecl     ::= "type" Identifier TypeParams? "=" TypeDef
TypeDef      ::= UnionDef | TypeExpr
UnionDef     ::= "|"? VariantDecl ("|" VariantDecl)*
VariantDecl  ::= Identifier ("(" (TypeExpr ("," TypeExpr)*)? ")")?
IfExpr       ::= "if" "(" Expr ")" Block ("else" (Block | IfExpr))?
UnaryExpr    ::= ("perform" | "check" | "!" | "-")* PostfixExpr
```

## 2. Semantics

- Pure by default: `perform` or calling an `IO`-returning function in a non-`IO` function is a type error. Pure functions cannot return `void` (dead computation is a semantic type error); effectful functions performing actions must return `IO(void)` or `IO(T)`.
- `check perform f()` on `IO(Result(T,E))` = run IO, then bubble `Err`. `perform check f()` on `Result(IO(T),E)` = validate first, skip IO on `Err`.
- Devirtualize trait calls when whole-program analysis finds one implementer. Vtable = methods + `drop` + `clone_rc`.

## 3. Pipeline

`chumsky` parse → typed AST → inference + trait resolution + purity check → desugar (`check`/`perform`/match) → ANF (`let _tN = ...`) + closure conversion (env struct + `function lambda_N(env, arg)`) → liveness/borrow → Perceus `inc/dec_ref` + FBIP → `inkwell` LLVM `-O3`.

## 4. Memory & backend

- No GC/RTS. Primitives unboxed; every heap object = `[u64 rc | payload]`.
- Borrowed args: no `inc/dec_ref`. Last use moves (zero RC traffic); shared use `inc_ref` at branches. `{ ...x, f: v }` reuses buffer iff `rc == 1`, else `dec + alloc + memcpy + rc = 1`.
- Threads: one async loop per OS thread, non-atomic RC, share-nothing. Channels: `rc == 1` → move pointer, `rc > 1` → deep clone.
- LLVM: internal `fastcc`, FFI/entry `ccc`; direct recursion `musttail`; inline `dec_ref` fast path (`sub` + `icmp eq 0` → `drop_fields` + `free`).

## 5. Roadmap

1. `chumsky` parser + `src/ast/` + Pratt exprs + `Result(T, E)` tests.
2. Symbol table + bidirectional inference + purity check + `check`/`perform` desugar.
3. ANF + liveness + `inc/dec_ref` + FBIP.
4. `inkwell` layouts + `musttail` + `malloc`/`free`/IO stubs.
