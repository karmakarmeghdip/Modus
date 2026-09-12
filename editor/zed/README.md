# Zed Editor Extension for Modus

This directory contains an extension package to integrate Modus language support (`.mds`) into the [Zed editor](https://zed.dev/).

## Features

- **Syntax Highlighting**: Functions, types, traits, immutable let bindings, parenthesized generics, structural records, closures, effects (`perform`), error bubbling (`check`), keywords, and literals.
- **Code Outline & Breadcrumbs**: Function, type, trait, and implementation symbols appear in Zed's outline view and breadcrumb navigation.
- **Bracket Matching & Autoclosing**: Automatic pairing and closing for `{}`, `[]`, `()`, and `""`.
- **Indentation Rules**: Auto-indent on block, record, match, parameter, and argument lists.
- **Comment Toggling**: Line comments (`//`) and block comments (`/* ... */`).

## Installation Instructions

### Option 1: Install as Dev Extension (Local Development)

1. Open Zed.
2. Press `Cmd+Shift+P` (macOS) or `Ctrl+Shift+P` (Linux/Windows) to open the Command Palette.
3. Type `zed: install dev extension` and press Enter.
4. In the file picker, select this directory (`editor/zed`).
5. Zed will load the Modus extension immediately. Open any `.mds` file to verify highlighting and navigation!

### Option 2: Point Zed Grammar to Local Tree-Sitter Repo

If you have cloned the Modus repository, Zed can reference the grammar directly:
```toml
[grammars.modus]
repository = "https://github.com/modus-lang/modus"
commit = "HEAD"
path = "editor"
```
