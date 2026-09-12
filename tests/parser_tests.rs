use modus::ast::*;
use modus::parser::*;
use std::fs;
use std::path::Path;

fn read_fixture(subpath: &str) -> String {
    let full_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(subpath);
    fs::read_to_string(&full_path).unwrap_or_else(|err| {
        panic!(
            "Failed to read fixture file {}: {}",
            full_path.display(),
            err
        )
    })
}

// ============================================================================
// Valid Fixture Tests: All valid .mds programs must parse successfully
// ============================================================================

#[test]
fn test_fixture_01_primitives() {
    let source = read_fixture("valid/01_primitives.mds");
    let result = parse_program(&source);
    assert!(
        result.is_ok(),
        "Expected valid/01_primitives.mds to parse, got: {:?}",
        result.err()
    );
    let program = result.unwrap();
    assert!(
        !program.declarations.is_empty(),
        "Expected declarations in 01_primitives.mds"
    );
}

#[test]
fn test_fixture_02_functions() {
    let source = read_fixture("valid/02_functions.mds");
    let result = parse_program(&source);
    assert!(
        result.is_ok(),
        "Expected valid/02_functions.mds to parse, got: {:?}",
        result.err()
    );
    let program = result.unwrap();
    assert!(!program.declarations.is_empty());
}

#[test]
fn test_fixture_03_control_flow() {
    let source = read_fixture("valid/03_control_flow.mds");
    let result = parse_program(&source);
    assert!(
        result.is_ok(),
        "Expected valid/03_control_flow.mds to parse, got: {:?}",
        result.err()
    );
    let program = result.unwrap();
    assert!(!program.declarations.is_empty());
}

#[test]
fn test_fixture_04_records() {
    let source = read_fixture("valid/04_records.mds");
    let result = parse_program(&source);
    assert!(
        result.is_ok(),
        "Expected valid/04_records.mds to parse, got: {:?}",
        result.err()
    );
    let program = result.unwrap();
    assert!(!program.declarations.is_empty());
}

#[test]
fn test_fixture_05_generics() {
    let source = read_fixture("valid/05_generics.mds");
    let result = parse_program(&source);
    assert!(
        result.is_ok(),
        "Expected valid/05_generics.mds to parse, got: {:?}",
        result.err()
    );
    let program = result.unwrap();
    assert!(!program.declarations.is_empty());
}

#[test]
fn test_fixture_06_traits() {
    let source = read_fixture("valid/06_traits.mds");
    let result = parse_program(&source);
    assert!(
        result.is_ok(),
        "Expected valid/06_traits.mds to parse, got: {:?}",
        result.err()
    );
    let program = result.unwrap();
    assert!(!program.declarations.is_empty());
}

#[test]
fn test_fixture_07_effects() {
    let source = read_fixture("valid/07_effects.mds");
    let result = parse_program(&source);
    assert!(
        result.is_ok(),
        "Expected valid/07_effects.mds to parse, got: {:?}",
        result.err()
    );
    let program = result.unwrap();
    assert!(!program.declarations.is_empty());
}

#[test]
fn test_fixture_08_match() {
    let source = read_fixture("valid/08_match.mds");
    let result = parse_program(&source);
    assert!(
        result.is_ok(),
        "Expected valid/08_match.mds to parse, got: {:?}",
        result.err()
    );
    let program = result.unwrap();
    assert!(!program.declarations.is_empty());
}

#[test]
fn test_fixture_09_closures() {
    let source = read_fixture("valid/09_closures.mds");
    let result = parse_program(&source);
    assert!(
        result.is_ok(),
        "Expected valid/09_closures.mds to parse, got: {:?}",
        result.err()
    );
    let program = result.unwrap();
    assert!(!program.declarations.is_empty());
}

#[test]
fn test_fixture_10_arrays() {
    let source = read_fixture("valid/10_arrays.mds");
    let result = parse_program(&source);
    assert!(
        result.is_ok(),
        "Expected valid/10_arrays.mds to parse, got: {:?}",
        result.err()
    );
    let program = result.unwrap();
    assert!(!program.declarations.is_empty());
}

#[test]
fn test_fixture_11_operators() {
    let source = read_fixture("valid/11_operators.mds");
    let result = parse_program(&source);
    assert!(
        result.is_ok(),
        "Expected valid/11_operators.mds to parse, got: {:?}",
        result.err()
    );
    let program = result.unwrap();
    assert!(!program.declarations.is_empty());
}

#[test]
fn test_fixture_12_full_program() {
    let source = read_fixture("valid/12_full_program.mds");
    let result = parse_program(&source);
    assert!(
        result.is_ok(),
        "Expected valid/12_full_program.mds to parse, got: {:?}",
        result.err()
    );
    let program = result.unwrap();
    assert!(!program.declarations.is_empty());
}

// ============================================================================
// Invalid Fixture Tests: Syntax violations must be rejected with parse errors
// ============================================================================

#[test]
fn test_invalid_01_fn_keyword() {
    let source = read_fixture("invalid/01_fn_keyword.mds");
    let result = parse_program(&source);
    assert!(
        result.is_err(),
        "Parser must reject 'fn' keyword (Modus requires 'function')"
    );
}

#[test]
fn test_invalid_02_angle_generics() {
    let source = read_fixture("invalid/02_angle_generics.mds");
    let result = parse_program(&source);
    assert!(
        result.is_err(),
        "Parser must reject angle bracket generics <T> (Modus uses (T))"
    );
}

#[test]
fn test_invalid_03_if_no_parentheses() {
    let source = read_fixture("invalid/03_if_no_parentheses.mds");
    let result = parse_program(&source);
    assert!(
        result.is_err(),
        "Parser must reject if conditions without parentheses"
    );
}

#[test]
fn test_invalid_04_dyn_keyword() {
    let source = read_fixture("invalid/04_dyn_keyword.mds");
    let result = parse_program(&source);
    assert!(result.is_err(), "Parser must reject 'dyn' keyword");
}

#[test]
fn test_invalid_05_missing_param_colon() {
    let source = read_fixture("invalid/05_missing_param_colon.mds");
    let result = parse_program(&source);
    assert!(
        result.is_err(),
        "Parser must reject parameter without colon and type"
    );
}

#[test]
fn test_invalid_06_mut_keyword() {
    let source = read_fixture("invalid/06_mut_keyword.mds");
    let result = parse_program(&source);
    assert!(
        result.is_err(),
        "Parser must reject 'mut' keyword (Modus variables are strictly immutable)"
    );
}

#[test]
fn test_invalid_07_while_loop() {
    let source = read_fixture("invalid/07_while_loop.mds");
    let result = parse_program(&source);
    assert!(
        result.is_err(),
        "Parser must reject 'while' loops (Modus has no loops; use recursion)"
    );
}

#[test]
fn test_invalid_08_for_loop() {
    let source = read_fixture("invalid/08_for_loop.mds");
    let result = parse_program(&source);
    assert!(
        result.is_err(),
        "Parser must reject 'for' loops (Modus has no loops; use recursion)"
    );
}

#[test]
fn test_invalid_09_struct_keyword() {
    let source = read_fixture("invalid/09_struct_keyword.mds");
    let result = parse_program(&source);
    assert!(
        result.is_err(),
        "Parser must reject 'struct' keyword (Modus uses 'type Point = {{ ... }}')"
    );
}

#[test]
fn test_invalid_10_enum_keyword() {
    let source = read_fixture("invalid/10_enum_keyword.mds");
    let result = parse_program(&source);
    assert!(
        result.is_err(),
        "Parser must reject 'enum' keyword (Modus uses 'type Option(T) = Some(T) | None')"
    );
}

#[test]
fn test_invalid_11_named_record_instantiation() {
    let source = read_fixture("invalid/11_named_record_instantiation.mds");
    let result = parse_program(&source);
    assert!(
        result.is_err(),
        "Parser must reject named record instantiation Point {{ ... }} (use structural {{ ... }})"
    );
}

// ============================================================================
// Fine-Grained Unit Tests: Grammar rules, precedence, and gotchas
// ============================================================================

#[test]
fn test_function_syntax_variants() {
    // Block body
    let src1 = "function add(a: i32, b: i32): i32 { return a + b; }";
    let res1 = parse_decl(src1);
    assert!(
        res1.is_ok(),
        "Block body function should parse: {:?}",
        res1.err()
    );
    if let Ok(spanned) = res1 {
        match spanned.node {
            Declaration::Function(f) => {
                assert_eq!(f.name, "add");
                assert_eq!(f.params.len(), 2);
                assert!(matches!(f.body, Some(FunctionBody::Block(_))));
            }
            _ => panic!("Expected Function declaration"),
        }
    }

    // Expression body with =>
    let src2 = "function double(x: i32): i32 => x * 2;";
    let res2 = parse_decl(src2);
    assert!(
        res2.is_ok(),
        "Expression body function should parse: {:?}",
        res2.err()
    );
    if let Ok(spanned) = res2 {
        match spanned.node {
            Declaration::Function(f) => {
                assert_eq!(f.name, "double");
                assert!(matches!(f.body, Some(FunctionBody::Expr(_))));
            }
            _ => panic!("Expected Function declaration"),
        }
    }

    // Generic parameters (parenthesized) and trait bounds
    let src3 = "function render(T: Drawable)(item: T): IO(void) { perform item.draw(); }";
    let res3 = parse_decl(src3);
    assert!(
        res3.is_ok(),
        "Generic bounded function should parse: {:?}",
        res3.err()
    );
    if let Ok(spanned) = res3 {
        match spanned.node {
            Declaration::Function(f) => {
                assert_eq!(f.type_params.len(), 1);
                assert_eq!(f.type_params[0].name, "T");
                assert!(f.type_params[0].bound.is_some());
            }
            _ => panic!("Expected Function declaration"),
        }
    }
}

#[test]
fn test_parenthesized_generics_not_confused_with_comparison() {
    // Result(T, E) must parse as a generic type, not comparisons
    let ty_src = "Result(i32, String)";
    let ty_res = parse_type(ty_src);
    assert!(
        ty_res.is_ok(),
        "Generic type Result(T, E) should parse: {:?}",
        ty_res.err()
    );
    if let Ok(spanned) = ty_res {
        match spanned.node {
            Type::Generic { name, type_args } => {
                assert_eq!(name, "Result");
                assert_eq!(type_args.len(), 2);
            }
            _ => panic!("Expected Type::Generic, got {:?}", spanned.node),
        }
    }

    // Nested generics
    let nested_src = "IO(Result(Option(i32), String))";
    let nested_res = parse_type(nested_src);
    assert!(
        nested_res.is_ok(),
        "Nested generic types should parse: {:?}",
        nested_res.err()
    );
}

#[test]
fn test_if_strictly_requires_parentheses() {
    // Valid: with parens
    let valid_if = "if (x > 0) { return 1; } else { return 0; }";
    let res_valid = parse_stmt(valid_if);
    assert!(
        res_valid.is_ok(),
        "Parenthesized if should parse: {:?}",
        res_valid.err()
    );

    // Valid: else if chain
    let valid_chain = "if (x > 0) { return 1; } else if (x < 0) { return -1; } else { return 0; }";
    let res_chain = parse_stmt(valid_chain);
    assert!(
        res_chain.is_ok(),
        "else if chain should parse: {:?}",
        res_chain.err()
    );

    // Invalid: without parens
    let invalid_if = "if x > 0 { return 1; }";
    let res_invalid = parse_stmt(invalid_if);
    assert!(res_invalid.is_err(), "Unparenthesized if must be rejected");
}

#[test]
fn test_pratt_precedence_prefix_unary_operators() {
    // check perform f() -> check(perform(f()))
    let src = "check perform f()";
    let res = parse_expr(src);
    assert!(
        res.is_ok(),
        "Prefix unary check perform should parse: {:?}",
        res.err()
    );
    if let Ok(spanned) = res {
        match spanned.node {
            Expr::Unary {
                op: UnaryOp::Check,
                expr: inner,
            } => match inner.node {
                Expr::Unary {
                    op: UnaryOp::Perform,
                    expr: call_expr,
                } => {
                    assert!(matches!(call_expr.node, Expr::Call { .. }));
                }
                _ => panic!("Expected perform inner expression, got {:?}", inner.node),
            },
            _ => panic!("Expected check outer expression, got {:?}", spanned.node),
        }
    }

    // -!x -> -(!x)
    let src2 = "-!x";
    let res2 = parse_expr(src2);
    assert!(res2.is_ok(), "Prefix -!x should parse: {:?}", res2.err());
    if let Ok(spanned) = res2 {
        match spanned.node {
            Expr::Unary {
                op: UnaryOp::Neg,
                expr: inner,
            } => {
                assert!(matches!(
                    inner.node,
                    Expr::Unary {
                        op: UnaryOp::Not,
                        ..
                    }
                ));
            }
            _ => panic!("Expected Neg outer expression"),
        }
    }
}

#[test]
fn test_pratt_precedence_binary_operators() {
    // 1 + 2 * 3 -> 1 + (2 * 3)
    let src = "1 + 2 * 3";
    let res = parse_expr(src);
    assert!(res.is_ok(), "1 + 2 * 3 should parse: {:?}", res.err());
    if let Ok(spanned) = res {
        match spanned.node {
            Expr::Binary {
                op: BinaryOp::Add,
                rhs,
                ..
            } => match rhs.node {
                Expr::Binary {
                    op: BinaryOp::Mul, ..
                } => {}
                _ => panic!("Expected Mul on rhs of Add, got {:?}", rhs.node),
            },
            _ => panic!("Expected Add at root, got {:?}", spanned.node),
        }
    }

    // a && b || c -> (a && b) || c
    let src2 = "a && b || c";
    let res2 = parse_expr(src2);
    assert!(res2.is_ok(), "a && b || c should parse: {:?}", res2.err());
    if let Ok(spanned) = res2 {
        match spanned.node {
            Expr::Binary {
                op: BinaryOp::Or,
                lhs,
                ..
            } => match lhs.node {
                Expr::Binary {
                    op: BinaryOp::And, ..
                } => {}
                _ => panic!("Expected And on lhs of Or, got {:?}", lhs.node),
            },
            _ => panic!("Expected Or at root, got {:?}", spanned.node),
        }
    }
}

#[test]
fn test_postfix_chaining() {
    // item.draw().len()[0]
    let src = "item.draw().len()[0]";
    let res = parse_expr(src);
    assert!(
        res.is_ok(),
        "Postfix chaining should parse: {:?}",
        res.err()
    );
    if let Ok(spanned) = res {
        match spanned.node {
            Expr::Index { receiver, .. } => match receiver.node {
                Expr::MethodCall { method, .. } => {
                    assert_eq!(method, "len");
                }
                _ => panic!("Expected MethodCall receiver of Index"),
            },
            _ => panic!("Expected Index at root, got {:?}", spanned.node),
        }
    }
}

#[test]
fn test_record_functional_update() {
    // { ...cfg, port: 8080 }
    let src = "{ ...cfg, port: 8080 }";
    let res = parse_expr(src);
    assert!(res.is_ok(), "Record update should parse: {:?}", res.err());
    if let Ok(spanned) = res {
        match spanned.node {
            Expr::RecordUpdate { base, fields } => {
                assert!(matches!(base.node, Expr::Ident(name) if name == "cfg"));
                assert_eq!(fields.len(), 1);
                assert_eq!(fields[0].0, "port");
            }
            _ => panic!("Expected RecordUpdate, got {:?}", spanned.node),
        }
    }
}

#[test]
fn test_trait_and_impl_syntax() {
    let trait_src = "trait Drawable(Self) { function draw(self: Self): IO(void); }";
    let res_trait = parse_decl(trait_src);
    assert!(
        res_trait.is_ok(),
        "Trait declaration should parse: {:?}",
        res_trait.err()
    );
    if let Ok(spanned) = res_trait {
        match spanned.node {
            Declaration::Trait(t) => {
                assert_eq!(t.name, "Drawable");
                assert_eq!(t.type_params.len(), 1);
                assert_eq!(t.members.len(), 1);
            }
            _ => panic!("Expected Trait declaration"),
        }
    }

    let impl_src = "impl Drawable for Circle { function draw(self: Circle): IO(void) {} }";
    let res_impl = parse_decl(impl_src);
    assert!(
        res_impl.is_ok(),
        "Impl declaration should parse: {:?}",
        res_impl.err()
    );
    if let Ok(spanned) = res_impl {
        match spanned.node {
            Declaration::Impl(i) => {
                assert_eq!(i.trait_name, "Drawable");
                assert_eq!(i.methods.len(), 1);
            }
            _ => panic!("Expected Impl declaration"),
        }
    }
}

#[test]
fn test_match_expression_with_various_patterns() {
    let src = r#"
    match shape {
        Shape.Circle(r) => r * 3.14,
        { w: width, h: height } => width * height,
        (x, y) => x + y,
        _ => 0.0,
    }
    "#;
    let res = parse_expr(src);
    assert!(
        res.is_ok(),
        "Match expression should parse: {:?}",
        res.err()
    );
    if let Ok(spanned) = res {
        match spanned.node {
            Expr::Match { expr: _, arms } => {
                assert_eq!(arms.len(), 4);
                assert!(matches!(arms[0].pattern.node, Pattern::Variant { .. }));
                assert!(matches!(arms[1].pattern.node, Pattern::Record(_)));
                assert!(matches!(arms[2].pattern.node, Pattern::Tuple(_)));
                assert!(matches!(arms[3].pattern.node, Pattern::Wildcard));
            }
            _ => panic!("Expected Match expr, got {:?}", spanned.node),
        }
    }
}

#[test]
fn test_closure_syntax() {
    let src1 = "(x: i32) => x + 1";
    let res1 = parse_expr(src1);
    assert!(
        res1.is_ok(),
        "Single param closure should parse: {:?}",
        res1.err()
    );

    let src2 = "(a: i32, b: i32): i32 => a + b";
    let res2 = parse_expr(src2);
    assert!(
        res2.is_ok(),
        "Multi param closure with return type should parse: {:?}",
        res2.err()
    );
}

#[test]
fn test_immutable_let_binding() {
    let src = "let count: i32 = 42;";
    let res = parse_stmt(src);
    assert!(res.is_ok(), "Immutable let should parse: {:?}", res.err());
    if let Ok(spanned) = res {
        match spanned.node {
            Stmt::Let {
                name,
                ty,
                initializer,
            } => {
                assert_eq!(name, "count");
                assert!(ty.is_some());
                assert!(matches!(initializer.node, Expr::Literal(Literal::Int(42))));
            }
            _ => panic!("Expected Stmt::Let"),
        }
    }
}

#[test]
fn test_reject_mut_keyword() {
    let src = "let mut x = 10;";
    let res = parse_stmt(src);
    assert!(
        res.is_err(),
        "Parser must reject 'mut' in let bindings (variables are always immutable)"
    );
}

#[test]
fn test_reject_loops() {
    let while_src = "while (x > 0) { x = x - 1; }";
    let res_while = parse_stmt(while_src);
    assert!(
        res_while.is_err(),
        "Parser must reject while loops (pure functional recursion only)"
    );

    let for_src = "for (item in list) { draw(item); }";
    let res_for = parse_stmt(for_src);
    assert!(
        res_for.is_err(),
        "Parser must reject for loops (pure functional recursion only)"
    );
}

#[test]
fn test_type_record_syntax() {
    let src = "type Point = { x: i32, y: i32 };";
    let res = parse_decl(src);
    assert!(
        res.is_ok(),
        "type record declaration should parse: {:?}",
        res.err()
    );
    if let Ok(spanned) = res {
        match spanned.node {
            Declaration::Type(t) => {
                assert_eq!(t.name, "Point");
                assert!(matches!(t.definition.node, TypeDef::Alias(Type::Record(_))));
            }
            _ => panic!("Expected Declaration::Type"),
        }
    }
}

#[test]
fn test_type_discriminated_union_syntax() {
    let src = "type Option(T) = Some(T) | None;";
    let res = parse_decl(src);
    assert!(
        res.is_ok(),
        "type discriminated union should parse: {:?}",
        res.err()
    );
    if let Ok(spanned) = res {
        match spanned.node {
            Declaration::Type(t) => {
                assert_eq!(t.name, "Option");
                assert_eq!(t.type_params.len(), 1);
                match t.definition.node {
                    TypeDef::Union(variants) => {
                        assert_eq!(variants.len(), 2);
                        assert_eq!(variants[0].name, "Some");
                        assert_eq!(variants[0].fields.len(), 1);
                        assert_eq!(variants[1].name, "None");
                        assert_eq!(variants[1].fields.len(), 0);
                    }
                    _ => panic!("Expected TypeDef::Union"),
                }
            }
            _ => panic!("Expected Declaration::Type"),
        }
    }
}

#[test]
fn test_reject_struct_keyword() {
    let src = "struct Point { x: i32, y: i32 }";
    let res = parse_decl(src);
    assert!(
        res.is_err(),
        "Parser must reject 'struct' keyword (use 'type Point = {{ ... }};')"
    );
}

#[test]
fn test_reject_enum_keyword() {
    let src = "enum Option(T) { Some(T), None }";
    let res = parse_decl(src);
    assert!(
        res.is_err(),
        "Parser must reject 'enum' keyword (use 'type Option(T) = Some(T) | None;')"
    );
}

#[test]
fn test_reject_named_record_instantiation() {
    let src = "Point { x: 1, y: 2 }";
    let res = parse_expr(src);
    assert!(
        res.is_err(),
        "Parser must reject named record instantiation 'Point {{ ... }}' (use '{{ ... }}' directly)"
    );
}

#[test]
fn test_structural_record_literal() {
    let src = "{ x: 1, y: 2 }";
    let res = parse_expr(src);
    assert!(
        res.is_ok(),
        "Structural record literal should parse: {:?}",
        res.err()
    );
    if let Ok(spanned) = res {
        match spanned.node {
            Expr::Record(fields) => {
                assert_eq!(fields.len(), 2);
                assert_eq!(fields[0].0, "x");
                assert_eq!(fields[1].0, "y");
            }
            _ => panic!("Expected Expr::Record, got {:?}", spanned.node),
        }
    }
}

#[test]
fn test_parse_extern_block() {
    let src = r#"
extern "C" {
    function puts(s: CString): IO(i32);
    function open(path: CString, flags: i32, mode: u32): IO(i32);
    function strlen(s: CString): u64;
}
"#;
    let res = parse_program(src);
    assert!(res.is_ok(), "Extern block should parse: {:?}", res.err());
    let prog = res.unwrap();
    assert_eq!(prog.declarations.len(), 1);
    match &prog.declarations[0].node {
        Declaration::Extern(ext) => {
            assert_eq!(ext.abi.as_deref(), Some("C"));
            assert_eq!(ext.functions.len(), 3);
            assert_eq!(ext.functions[0].node.name, "puts");
            assert_eq!(ext.functions[1].node.name, "open");
            assert_eq!(ext.functions[2].node.name, "strlen");
        }
        _ => panic!("Expected Declaration::Extern"),
    }
}

#[test]
fn test_parse_extern_single_function() {
    let src = r#"
extern "libc.so.6" function malloc(size: u64): IO(Pointer(void));
"#;
    let res = parse_program(src);
    assert!(
        res.is_ok(),
        "Single extern function should parse: {:?}",
        res.err()
    );
    let prog = res.unwrap();
    assert_eq!(prog.declarations.len(), 1);
    match &prog.declarations[0].node {
        Declaration::Extern(ext) => {
            assert_eq!(ext.abi.as_deref(), Some("libc.so.6"));
            assert_eq!(ext.functions.len(), 1);
            assert_eq!(ext.functions[0].node.name, "malloc");
        }
        _ => panic!("Expected Declaration::Extern"),
    }
}
