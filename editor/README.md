# tree-sitter-modus

Tree-sitter grammar for the Modus programming language (`.mds`).

## Overview

Modus is a pure-by-default systems language featuring:
- TypeScript ergonomics (`function`, `type`, structural records `{ x: 1 }`, arrow closures `(x: T) => expr`)
- Haskell purity (`IO`, `perform`)
- Rust primitives and immutability (no `mut`, strictly immutable `let`, no loops `while`/`for`)
- Parenthesized generics: `Result(T, E)`, `Option(T)`
- Perceus reference counting and tail call recursion

This package provides a complete Tree-sitter grammar and editor query files (`highlights.scm`, `indents.scm`, `folds.scm`, `textobjects.scm`, `locals.scm`).

## Commands

- Generate parser: `npx tree-sitter-cli generate`
- Run test corpus: `npx tree-sitter-cli test`
- Parse file: `npx tree-sitter-cli parse <file.mds>`
- Rust tests: `cargo test`

## Directory Layout

- `grammar.js`: Language grammar definition
- `tree-sitter.json`: Tree-sitter 0.22+ package metadata
- `queries/`: Tree-sitter queries:
  - `highlights.scm`: Syntax highlighting
  - `indents.scm`: Indentation rules
  - `folds.scm`: Code folding
  - `textobjects.scm`: Structural selection motions
  - `locals.scm`: Scope and local definition tracking
- `test/corpus/`: Grammar test corpus
- `helix/`: Helix editor integration files
- `zed/`: Zed editor extension and integration files
