//! Document representation, line index, and thread-safe document store.

use crate::ast::{Program, Span};
use crate::modules::ModuleInterface;
use crate::typechecker::Environment;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use tower_lsp::lsp_types::{Diagnostic, Position, Range, Url};

/// Fast bidirectional mapping between 1D byte offsets and 2D LSP (line, character) positions.
#[derive(Debug, Clone)]
pub struct LineIndex {
    line_offsets: Vec<usize>,
    len: usize,
}

impl LineIndex {
    pub fn new(text: &str) -> Self {
        let mut line_offsets = vec![0];
        for (i, byte) in text.bytes().enumerate() {
            if byte == b'\n' {
                line_offsets.push(i + 1);
            }
        }
        Self {
            line_offsets,
            len: text.len(),
        }
    }

    /// Converts a byte offset into an LSP Position (0-indexed line and character).
    pub fn offset_to_position(&self, offset: usize) -> Position {
        let offset = offset.min(self.len);
        let line = match self.line_offsets.binary_search(&offset) {
            Ok(line) => line,
            Err(next_line) => next_line.saturating_sub(1),
        };
        let line_start = self.line_offsets[line];
        let character = offset.saturating_sub(line_start);

        Position {
            line: line as u32,
            character: character as u32,
        }
    }

    /// Converts an LSP Position into a byte offset.
    pub fn position_to_offset(&self, position: Position) -> usize {
        let line = position.line as usize;
        if line >= self.line_offsets.len() {
            return self.len;
        }
        let line_start = self.line_offsets[line];
        let next_line_start = self.line_offsets.get(line + 1).copied().unwrap_or(self.len);
        let offset = line_start + (position.character as usize);
        offset.min(next_line_start).min(self.len)
    }

    /// Maps an AST byte Span into an LSP Range.
    pub fn span_to_range(&self, span: Span) -> Range {
        Range {
            start: self.offset_to_position(span.start),
            end: self.offset_to_position(span.end),
        }
    }

    /// Maps an LSP Range into an AST byte Span.
    pub fn range_to_span(&self, range: Range) -> Span {
        Span::new(
            self.position_to_offset(range.start),
            self.position_to_offset(range.end),
        )
    }
}

/// In-memory representation of an open Modus source file.
#[derive(Debug, Clone)]
pub struct Document {
    pub uri: Url,
    pub version: i32,
    pub text: String,
    pub line_index: LineIndex,
    pub program: Option<Program>,
    pub env: Option<Environment>,
    pub interface: Option<ModuleInterface>,
    pub diagnostics: Vec<Diagnostic>,
}

impl Document {
    pub fn new(uri: Url, version: i32, text: String) -> Self {
        let line_index = LineIndex::new(&text);
        Self {
            uri,
            version,
            text,
            line_index,
            program: None,
            env: None,
            interface: None,
            diagnostics: Vec::new(),
        }
    }

    pub fn update(&mut self, version: i32, text: String) {
        self.version = version;
        self.line_index = LineIndex::new(&text);
        self.text = text;
        self.program = None;
        self.env = None;
        self.interface = None;
        self.diagnostics.clear();
    }
}

/// Thread-safe map of open documents keyed by document URL and module interface cache.
#[derive(Debug, Default, Clone)]
pub struct DocumentStore {
    documents: Arc<RwLock<HashMap<Url, Document>>>,
    module_cache: Arc<RwLock<HashMap<PathBuf, ModuleInterface>>>,
}

impl DocumentStore {
    pub fn new() -> Self {
        Self {
            documents: Arc::new(RwLock::new(HashMap::new())),
            module_cache: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub fn insert(&self, uri: Url, version: i32, text: String) -> Document {
        let doc = Document::new(uri.clone(), version, text);
        self.documents.write().unwrap().insert(uri, doc.clone());
        doc
    }

    pub fn update(&self, uri: &Url, version: i32, text: String) -> Option<Document> {
        let mut docs = self.documents.write().unwrap();
        if let Some(doc) = docs.get_mut(uri) {
            doc.update(version, text);
            Some(doc.clone())
        } else {
            let doc = Document::new(uri.clone(), version, text);
            docs.insert(uri.clone(), doc.clone());
            Some(doc)
        }
    }

    pub fn get(&self, uri: &Url) -> Option<Document> {
        self.documents.read().unwrap().get(uri).cloned()
    }

    pub fn get_by_path(&self, path: &Path) -> Option<Document> {
        let docs = self.documents.read().unwrap();
        if let Ok(url) = Url::from_file_path(path)
            && let Some(doc) = docs.get(&url)
        {
            return Some(doc.clone());
        }
        if let Ok(canon) = std::fs::canonicalize(path)
            && let Ok(url) = Url::from_file_path(&canon)
            && let Some(doc) = docs.get(&url)
        {
            return Some(doc.clone());
        }
        None
    }

    pub fn get_text_by_path(&self, path: &Path) -> Option<String> {
        self.get_by_path(path).map(|d| d.text)
    }

    pub fn remove(&self, uri: &Url) -> Option<Document> {
        self.documents.write().unwrap().remove(uri)
    }

    pub fn update_analysis(
        &self,
        uri: &Url,
        program: Option<Program>,
        env: Option<Environment>,
        interface: Option<ModuleInterface>,
        diagnostics: Vec<Diagnostic>,
    ) {
        let mut docs = self.documents.write().unwrap();
        if let Some(doc) = docs.get_mut(uri) {
            doc.program = program;
            doc.env = env;
            doc.interface = interface;
            doc.diagnostics = diagnostics;
        }
    }

    pub fn get_cached_interface(&self, path: &Path) -> Option<ModuleInterface> {
        self.module_cache.read().unwrap().get(path).cloned()
    }

    pub fn cache_interface(&self, path: PathBuf, iface: ModuleInterface) {
        self.module_cache.write().unwrap().insert(path, iface);
    }

    pub fn invalidate_cache(&self, path: &Path) {
        self.module_cache.write().unwrap().remove(path);
    }

    pub fn get_all_open_docs(&self) -> Vec<Document> {
        self.documents.read().unwrap().values().cloned().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_line_index_basic() {
        let text = "function add(a: i32, b: i32): i32 {\n    return a + b;\n}\n";
        let idx = LineIndex::new(text);

        // Position of 'return' on line 1, char 4
        let pos = idx.offset_to_position(text.find("return").unwrap());
        assert_eq!(pos.line, 1);
        assert_eq!(pos.character, 4);

        let offset = idx.position_to_offset(pos);
        assert_eq!(offset, text.find("return").unwrap());
    }

    #[test]
    fn test_span_to_range() {
        let text = "let x = 42;\nlet y = 10;";
        let idx = LineIndex::new(text);

        let span = Span::new(0, 3); // "let"
        let range = idx.span_to_range(span);
        assert_eq!(
            range.start,
            Position {
                line: 0,
                character: 0
            }
        );
        assert_eq!(
            range.end,
            Position {
                line: 0,
                character: 3
            }
        );
    }
}
