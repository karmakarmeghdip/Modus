use modus::lsp::code_actions::code_actions_at;
use modus::lsp::completion::completions_at;
use modus::lsp::definition::definition_at;
use modus::lsp::diagnostics::compute_diagnostics;
use modus::lsp::document::{Document, LineIndex};
use modus::lsp::hover::hover_at;
use modus::lsp::symbols::document_symbols;
use tower_lsp::lsp_types::*;

#[test]
fn test_lsp_diagnostics_clean_on_valid_file() {
    let source = r#"
type Point = { x: i32, y: i32 };

function distance(p: Point): i32 {
    return p.x + p.y;
}
"#;
    let line_index = LineIndex::new(source);
    let (program, env, diags) = compute_diagnostics(source, &line_index);

    assert!(program.is_some(), "Program should parse successfully");
    assert!(env.is_some(), "Environment should typecheck successfully");
    assert!(
        diags.is_empty(),
        "Valid code should produce 0 diagnostics, found: {:?}",
        diags
    );
}

#[test]
fn test_lsp_diagnostics_on_forbidden_syntax() {
    let source = "fn add(a: i32): i32 { return a; }"; // 'fn' is rejected
    let line_index = LineIndex::new(source);
    let (program, _env, diags) = compute_diagnostics(source, &line_index);

    assert!(
        program.is_none(),
        "Invalid syntax should not produce an AST"
    );
    assert!(
        !diags.is_empty(),
        "Diagnostics must report the syntax error"
    );
    assert_eq!(diags[0].severity, Some(DiagnosticSeverity::ERROR));
}

#[test]
fn test_lsp_diagnostics_on_purity_violation() {
    let source = r#"
function pure_func(): i32 {
    perform IO.pure(42);
    return 42;
}
"#;
    let line_index = LineIndex::new(source);
    let (program, _env, diags) = compute_diagnostics(source, &line_index);

    assert!(program.is_some(), "Program parses");
    assert!(
        !diags.is_empty(),
        "Purity violation must produce a diagnostic"
    );
    assert!(
        diags[0].message.to_lowercase().contains("purity")
            || diags[0].message.to_lowercase().contains("perform"),
        "Diagnostic message should describe purity violation: {}",
        diags[0].message
    );
}

#[test]
fn test_lsp_document_symbols() {
    let source = r#"
type Shape = Circle(f64) | Rect(f64, f64);

trait Drawable(Self) {
    function draw(self: Self): IO(void);
}

function render(s: Shape): IO(void) {
    return IO.pure(());
}
"#;
    let line_index = LineIndex::new(source);
    let (program, _, _) = compute_diagnostics(source, &line_index);
    let symbols = document_symbols(&program.unwrap(), &line_index).expect("Should extract symbols");

    match symbols {
        DocumentSymbolResponse::Nested(syms) => {
            assert_eq!(syms.len(), 3);
            assert_eq!(syms[0].name, "Shape");
            assert_eq!(syms[0].kind, SymbolKind::ENUM);
            assert_eq!(syms[1].name, "Drawable");
            assert_eq!(syms[1].kind, SymbolKind::INTERFACE);
            assert_eq!(syms[2].name, "render");
            assert_eq!(syms[2].kind, SymbolKind::FUNCTION);
        }
        _ => panic!("Expected nested document symbols"),
    }
}

#[test]
fn test_lsp_hover_information() {
    let source = r#"
type Point = { x: i32, y: i32 };

function manhattan(p: Point): i32 {
    let total = p.x + p.y;
    return total;
}
"#;
    let uri = Url::parse("file:///test.mds").unwrap();
    let mut doc = Document::new(uri, 1, source.to_string());
    let (program, env, diags) = compute_diagnostics(source, &doc.line_index);
    doc.program = program;
    doc.env = env;
    doc.diagnostics = diags;

    // Hover on 'function' keyword (line 3, char 2)
    let hover_kw = hover_at(
        &doc,
        Position {
            line: 3,
            character: 2,
        },
    )
    .expect("Hover on keyword");
    if let HoverContents::Markup(m) = hover_kw.contents {
        assert!(
            m.value.contains("function"),
            "Markdown should describe keyword"
        );
    } else {
        panic!("Expected markup contents");
    }

    // Hover on 'manhattan' function (line 3, char 12)
    let hover_fn = hover_at(
        &doc,
        Position {
            line: 3,
            character: 12,
        },
    )
    .expect("Hover on function");
    if let HoverContents::Markup(m) = hover_fn.contents {
        assert!(
            m.value.contains("manhattan"),
            "Should show function signature"
        );
        assert!(m.value.contains("Pure"), "Should display purity annotation");
    } else {
        panic!("Expected markup contents");
    }
}

#[test]
fn test_lsp_goto_definition() {
    let source = r#"
function compute(a: i32): i32 {
    let delta = 10;
    return a + delta;
}
"#;
    let uri = Url::parse("file:///test.mds").unwrap();
    let mut doc = Document::new(uri, 1, source.to_string());
    let (program, env, diags) = compute_diagnostics(source, &doc.line_index);
    doc.program = program;
    doc.env = env;
    doc.diagnostics = diags;

    // Go to definition of 'compute' from invocation or name (line 1, char 12)
    let def_fn = definition_at(
        &doc,
        Position {
            line: 1,
            character: 12,
        },
    )
    .expect("Definition for function");
    match def_fn {
        GotoDefinitionResponse::Scalar(loc) => {
            assert_eq!(loc.range.start.line, 1);
        }
        _ => panic!("Expected scalar definition location"),
    }
}

#[test]
fn test_lsp_completions() {
    let source = "function test(): i32 { return 0; }";
    let uri = Url::parse("file:///test.mds").unwrap();
    let doc = Document::new(uri, 1, source.to_string());

    let completions = completions_at(
        &doc,
        Position {
            line: 0,
            character: 0,
        },
    )
    .expect("Completions list");
    match completions {
        CompletionResponse::List(list) => {
            let labels: Vec<&str> = list.items.iter().map(|i| i.label.as_str()).collect();
            assert!(
                labels.contains(&"function"),
                "Should complete keyword 'function'"
            );
            assert!(
                labels.contains(&"perform"),
                "Should complete keyword 'perform'"
            );
            assert!(labels.contains(&"check"), "Should complete keyword 'check'");
            assert!(labels.contains(&"i32"), "Should complete type 'i32'");
            assert!(labels.contains(&"IO"), "Should complete type 'IO'");
            assert!(
                labels.contains(&"Some"),
                "Should complete variant constructor 'Some'"
            );
        }
        _ => panic!("Expected completion list"),
    }
}

#[test]
fn test_lsp_code_actions_fn_to_function() {
    let source = "fn hello(): i32 { return 42; }";
    let uri = Url::parse("file:///test.mds").unwrap();
    let mut doc = Document::new(uri, 1, source.to_string());
    let (_, _, diags) = compute_diagnostics(source, &doc.line_index);
    doc.diagnostics = diags.clone();

    let context = CodeActionContext {
        diagnostics: diags,
        only: None,
        trigger_kind: None,
    };

    let actions = code_actions_at(&doc, Range::default(), &context).expect("Code actions response");
    assert!(!actions.is_empty(), "Should generate quick fix for 'fn'");

    if let CodeActionOrCommand::CodeAction(action) = &actions[0] {
        assert!(action.title.contains("function"));
        assert_eq!(action.kind, Some(CodeActionKind::QUICKFIX));
    } else {
        panic!("Expected code action");
    }
}
