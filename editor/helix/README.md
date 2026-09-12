# Helix Editor Integration for Modus

This directory contains configuration and tree-sitter queries to integrate Modus language support (`.mds`) into the [Helix editor](https://helix-editor.com/).

## Setup Instructions

### 1. Configure Language in Helix

Add the contents of `languages.toml` to your Helix configuration file (`~/.config/helix/languages.toml`):

```toml
[language-server.modus-lsp]
command = "modus"
args = ["lsp"]

[[language]]
name = "modus"
scope = "source.modus"
injection-regex = "modus"
file-types = ["mds"]
comment-token = "//"
block-comment-tokens = { start = "/*", end = "*/" }
indent = { tab-width = 4, unit = "    " }
roots = ["Cargo.toml", ".git"]
language-servers = ["modus-lsp"]

[[grammar]]
name = "modus"
source = { path = "/absolute/path/to/Modus/editor" }
```

> **Note**: Replace `/absolute/path/to/Modus/editor` with the absolute path to the `editor/` directory in your clone of the Modus repository.

### 2. Copy or Symlink Query Files

Helix looks for runtime queries in `~/.config/helix/runtime/queries/` (or your Helix runtime folder).

Link or copy the `queries/modus` directory:

```bash
mkdir -p ~/.config/helix/runtime/queries
ln -s "$(pwd)/queries/modus" ~/.config/helix/runtime/queries/modus
```

Alternatively, copy the files:

```bash
mkdir -p ~/.config/helix/runtime/queries/modus
cp queries/modus/* ~/.config/helix/runtime/queries/modus/
```

### 3. Build the Tree-sitter Grammar

In your terminal, tell Helix to fetch and build the Modus grammar:

```bash
hx --grammar build
```

Verify that the grammar is recognized:

```bash
hx --health modus
```

You should see green checks for syntax highlighting, indentation, folding, and textobjects.
