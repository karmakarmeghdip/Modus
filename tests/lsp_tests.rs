use modus::lsp::code_actions::code_actions_at;
use modus::lsp::completion::{completions_at, completions_with_store};
use modus::lsp::definition::{definition_at, definition_with_store};
use modus::lsp::diagnostics::{compute_diagnostics, compute_diagnostics_with_imports};
use modus::lsp::document::{Document, DocumentStore, LineIndex};
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

#[test]
fn test_lsp_cross_file_imports_clean() {
    let store = DocumentStore::new();

    let math_url = Url::parse("file:///workspace/math.mds").unwrap();
    let math_src = r#"
export type Point = { x: i32, y: i32 };

export function add(a: i32, b: i32): i32 {
    return a + b;
}
"#;
    let math_doc = store.insert(math_url.clone(), 1, math_src.to_string());
    let (m_prog, m_env, m_iface, m_diags) = compute_diagnostics_with_imports(
        math_src,
        &math_doc.line_index,
        Some(&math_url),
        Some(&store),
    );
    assert!(m_diags.is_empty(), "math.mds should have 0 diagnostics");
    store.update_analysis(&math_url, m_prog, m_env, m_iface, m_diags);

    let app_url = Url::parse("file:///workspace/app.mds").unwrap();
    let app_src = r#"
import { add, Point } from "./math.mds";

function main(): i32 {
    let p: Point = { x: 10, y: 20 };
    return add(p.x, p.y);
}
"#;
    let app_doc = store.insert(app_url.clone(), 1, app_src.to_string());
    let (a_prog, a_env, _a_iface, a_diags) = compute_diagnostics_with_imports(
        app_src,
        &app_doc.line_index,
        Some(&app_url),
        Some(&store),
    );

    assert!(a_prog.is_some(), "app.mds should parse successfully");
    assert!(a_env.is_some(), "app.mds should typecheck successfully");
    assert!(
        a_diags.is_empty(),
        "app.mds should have 0 diagnostics with imported symbols, found: {:?}",
        a_diags
    );
}

#[test]
fn test_lsp_cross_file_type_mismatch_on_imported_function() {
    let store = DocumentStore::new();

    let math_url = Url::parse("file:///workspace/math.mds").unwrap();
    let math_src = r#"
export function calculate(a: i32, b: i32): i32 {
    return a + b;
}
"#;
    let math_doc = store.insert(math_url.clone(), 1, math_src.to_string());
    let (m_prog, m_env, m_iface, m_diags) = compute_diagnostics_with_imports(
        math_src,
        &math_doc.line_index,
        Some(&math_url),
        Some(&store),
    );
    store.update_analysis(&math_url, m_prog, m_env, m_iface, m_diags);

    let app_url = Url::parse("file:///workspace/app.mds").unwrap();
    let app_src = r#"
import { calculate } from "./math.mds";

function main(): i32 {
    return calculate("invalid_string", 42);
}
"#;
    let app_doc = store.insert(app_url.clone(), 1, app_src.to_string());
    let (_prog, _env, _iface, diags) = compute_diagnostics_with_imports(
        app_src,
        &app_doc.line_index,
        Some(&app_url),
        Some(&store),
    );

    assert!(
        !diags.is_empty(),
        "Passing wrong argument type to imported function must produce a diagnostic"
    );
    assert_eq!(diags[0].severity, Some(DiagnosticSeverity::ERROR));
}

#[test]
fn test_lsp_cross_file_unexported_symbol_diagnostic() {
    let store = DocumentStore::new();

    let math_url = Url::parse("file:///workspace/math.mds").unwrap();
    let math_src = r#"
export function helper(): i32 {
    return 1;
}
"#;
    let math_doc = store.insert(math_url.clone(), 1, math_src.to_string());
    let (m_prog, m_env, m_iface, m_diags) = compute_diagnostics_with_imports(
        math_src,
        &math_doc.line_index,
        Some(&math_url),
        Some(&store),
    );
    store.update_analysis(&math_url, m_prog, m_env, m_iface, m_diags);

    let app_url = Url::parse("file:///workspace/app.mds").unwrap();
    let app_src = r#"
import { nonExistent } from "./math.mds";

function main(): i32 {
    return 0;
}
"#;
    let app_doc = store.insert(app_url.clone(), 1, app_src.to_string());
    let (_prog, _env, _iface, diags) = compute_diagnostics_with_imports(
        app_src,
        &app_doc.line_index,
        Some(&app_url),
        Some(&store),
    );

    assert!(
        !diags.is_empty(),
        "Importing unexported symbol must produce an error diagnostic"
    );
    assert!(
        diags[0].message.contains("nonExistent") || diags[0].message.contains("not exported"),
        "Diagnostic should mention unexported symbol: {}",
        diags[0].message
    );
}

#[test]
fn test_lsp_cross_file_goto_definition() {
    let store = DocumentStore::new();

    let math_url = Url::parse("file:///workspace/math.mds").unwrap();
    let math_src = r#"
export function calculate(a: i32): i32 {
    return a * 2;
}
"#;
    let math_doc = store.insert(math_url.clone(), 1, math_src.to_string());
    let (m_prog, m_env, m_iface, m_diags) = compute_diagnostics_with_imports(
        math_src,
        &math_doc.line_index,
        Some(&math_url),
        Some(&store),
    );
    store.update_analysis(&math_url, m_prog, m_env, m_iface, m_diags);

    let app_url = Url::parse("file:///workspace/app.mds").unwrap();
    let app_src = r#"
import { calculate } from "./math.mds";

function main(): i32 {
    return calculate(21);
}
"#;
    let app_doc = store.insert(app_url.clone(), 1, app_src.to_string());
    let (a_prog, a_env, a_iface, a_diags) = compute_diagnostics_with_imports(
        app_src,
        &app_doc.line_index,
        Some(&app_url),
        Some(&store),
    );
    store.update_analysis(&app_url, a_prog, a_env, a_iface, a_diags);

    let updated_app_doc = store.get(&app_url).unwrap();

    // Go to definition of 'calculate' from invocation site: line 4, char 13 ("calculate")
    let def_resp = definition_with_store(
        &updated_app_doc,
        Position {
            line: 4,
            character: 13,
        },
        Some(&store),
    )
    .expect("Go to definition for imported calculate");

    match def_resp {
        GotoDefinitionResponse::Scalar(loc) => {
            assert_eq!(loc.uri, math_url, "Must jump to math.mds");
            assert_eq!(
                loc.range.start.line, 1,
                "Must jump to calculate declaration on line 1 of math.mds"
            );
        }
        _ => panic!("Expected scalar location"),
    }

    // Go to definition from import specifier: line 1, char 11 ("calculate")
    let def_import = definition_with_store(
        &updated_app_doc,
        Position {
            line: 1,
            character: 11,
        },
        Some(&store),
    )
    .expect("Go to definition for import specifier");

    match def_import {
        GotoDefinitionResponse::Scalar(loc) => {
            assert_eq!(loc.uri, math_url, "Must jump to math.mds");
            assert_eq!(loc.range.start.line, 1);
        }
        _ => panic!("Expected scalar location"),
    }
}

#[test]
fn test_lsp_cross_file_hover_and_completion() {
    let store = DocumentStore::new();

    let math_url = Url::parse("file:///workspace/math.mds").unwrap();
    let math_src = r#"
export type Vector = { x: i32, y: i32 };

export function dotProduct(v1: Vector, v2: Vector): i32 {
    return v1.x * v2.x + v1.y * v2.y;
}
"#;
    let math_doc = store.insert(math_url.clone(), 1, math_src.to_string());
    let (m_prog, m_env, m_iface, m_diags) = compute_diagnostics_with_imports(
        math_src,
        &math_doc.line_index,
        Some(&math_url),
        Some(&store),
    );
    store.update_analysis(&math_url, m_prog, m_env, m_iface, m_diags);

    let app_url = Url::parse("file:///workspace/app.mds").unwrap();
    let app_src = r#"
import { dotProduct, Vector } from "./math.mds";

function main(): i32 {
    let v: Vector = { x: 1, y: 2 };
    return dotProduct(v, v);
}
"#;
    let app_doc = store.insert(app_url.clone(), 1, app_src.to_string());
    let (a_prog, a_env, a_iface, a_diags) = compute_diagnostics_with_imports(
        app_src,
        &app_doc.line_index,
        Some(&app_url),
        Some(&store),
    );
    store.update_analysis(&app_url, a_prog, a_env, a_iface, a_diags);

    let updated_app_doc = store.get(&app_url).unwrap();

    // Hover on 'dotProduct' on line 5, char 13
    let hover_res = hover_at(
        &updated_app_doc,
        Position {
            line: 5,
            character: 13,
        },
    )
    .expect("Hover on imported dotProduct");

    if let HoverContents::Markup(m) = hover_res.contents {
        assert!(
            m.value.contains("dotProduct"),
            "Should show function signature"
        );
        assert!(m.value.contains("Vector"), "Should show parameter types");
        assert!(m.value.contains("Pure"), "Should display pure annotation");
    } else {
        panic!("Expected markup hover");
    }

    // Completions in app.mds should include imported 'dotProduct' and 'Vector'
    let comp_res = completions_with_store(
        &updated_app_doc,
        Position {
            line: 5,
            character: 0,
        },
        Some(&store),
    )
    .expect("Completions in app.mds");

    if let CompletionResponse::List(list) = comp_res {
        let labels: Vec<&str> = list.items.iter().map(|i| i.label.as_str()).collect();
        assert!(
            labels.contains(&"dotProduct"),
            "Completions must include imported function 'dotProduct'"
        );
        assert!(
            labels.contains(&"Vector"),
            "Completions must include imported type 'Vector'"
        );
    } else {
        panic!("Expected completion list");
    }
}

#[test]
fn test_lsp_namespace_import_and_definition() {
    let store = DocumentStore::new();

    let math_url = Url::parse("file:///workspace/math.mds").unwrap();
    let math_src = r#"
export function square(n: i32): i32 {
    return n * n;
}
"#;
    let math_doc = store.insert(math_url.clone(), 1, math_src.to_string());
    let (m_prog, m_env, m_iface, m_diags) = compute_diagnostics_with_imports(
        math_src,
        &math_doc.line_index,
        Some(&math_url),
        Some(&store),
    );
    store.update_analysis(&math_url, m_prog, m_env, m_iface, m_diags);

    let app_url = Url::parse("file:///workspace/app.mds").unwrap();
    let app_src = r#"
import * as Math from "./math.mds";

function main(): i32 {
    return Math.square(9);
}
"#;
    let app_doc = store.insert(app_url.clone(), 1, app_src.to_string());
    let (a_prog, a_env, a_iface, a_diags) = compute_diagnostics_with_imports(
        app_src,
        &app_doc.line_index,
        Some(&app_url),
        Some(&store),
    );
    assert!(
        a_diags.is_empty(),
        "Namespace import should produce 0 diagnostics, got: {:?}",
        a_diags
    );
    store.update_analysis(&app_url, a_prog, a_env, a_iface, a_diags);

    let updated_app_doc = store.get(&app_url).unwrap();

    // Go to definition of 'square' in 'Math.square(9)' (line 4, char 18)
    let def_resp = definition_with_store(
        &updated_app_doc,
        Position {
            line: 4,
            character: 18,
        },
        Some(&store),
    )
    .expect("Go to definition on namespace-qualified square");

    match def_resp {
        GotoDefinitionResponse::Scalar(loc) => {
            assert_eq!(loc.uri, math_url);
            assert_eq!(loc.range.start.line, 1);
        }
        _ => panic!("Expected scalar location"),
    }
}
