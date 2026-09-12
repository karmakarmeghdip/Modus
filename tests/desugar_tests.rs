use modus::desugar::*;
use modus::parser::parse_program;
use modus::typechecker::check_program;
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
// Valid Fixtures Desugaring: All 12 valid fixture programs must desugar cleanly
// ============================================================================

#[test]
fn test_desugar_valid_01_primitives() {
    let source = read_fixture("valid/01_primitives.mds");
    let program = parse_program(&source).expect("Parsing failed");
    let env = check_program(&program).expect("Typecheck failed");
    let desugared = desugar_program(&program, &env);
    assert!(!desugared.declarations.is_empty());
}

#[test]
fn test_desugar_valid_02_functions() {
    let source = read_fixture("valid/02_functions.mds");
    let program = parse_program(&source).expect("Parsing failed");
    let env = check_program(&program).expect("Typecheck failed");
    let desugared = desugar_program(&program, &env);
    assert!(!desugared.declarations.is_empty());

    // Verify expression body function `square` was canonicalized to block with Return
    let square_fn = desugared
        .declarations
        .iter()
        .find_map(|d| match d {
            DesugaredDecl::Function(f) if f.name == "square" => Some(f),
            _ => None,
        })
        .expect("Function square must exist");

    assert!(
        matches!(
            square_fn.body.last(),
            Some(DesugaredStmt::Return(Some(_), _))
        ),
        "Expression body must be canonicalized to block ending with Return"
    );
}

#[test]
fn test_desugar_valid_03_control_flow() {
    let source = read_fixture("valid/03_control_flow.mds");
    let program = parse_program(&source).expect("Parsing failed");
    let env = check_program(&program).expect("Typecheck failed");
    let desugared = desugar_program(&program, &env);
    assert!(!desugared.declarations.is_empty());
}

#[test]
fn test_desugar_valid_04_records() {
    let source = read_fixture("valid/04_records.mds");
    let program = parse_program(&source).expect("Parsing failed");
    let env = check_program(&program).expect("Typecheck failed");
    let desugared = desugar_program(&program, &env);
    assert!(!desugared.declarations.is_empty());

    // Verify update_port desugared { ...cfg, port: new_port } into a full Record literal
    let update_port_fn = desugared
        .declarations
        .iter()
        .find_map(|d| match d {
            DesugaredDecl::Function(f) if f.name == "update_port" => Some(f),
            _ => None,
        })
        .expect("update_port function must exist");

    let ret_stmt = update_port_fn
        .body
        .last()
        .expect("Must have return statement");
    if let DesugaredStmt::Return(Some(ret_expr), _) = ret_stmt {
        if let DesugaredExprKind::Record(fields) = &ret_expr.kind {
            let field_names: Vec<&str> = fields.iter().map(|(n, _)| n.as_str()).collect();
            assert!(
                field_names.contains(&"host")
                    && field_names.contains(&"port")
                    && field_names.contains(&"tls"),
                "Record update must be lowered to full record literal containing all original fields: {:?}",
                field_names
            );
        } else {
            panic!(
                "Expected DesugaredExprKind::Record, got {:?}",
                ret_expr.kind
            );
        }
    } else {
        panic!("Expected Return with expr");
    }
}

#[test]
fn test_desugar_valid_05_generics() {
    let source = read_fixture("valid/05_generics.mds");
    let program = parse_program(&source).expect("Parsing failed");
    let env = check_program(&program).expect("Typecheck failed");
    let desugared = desugar_program(&program, &env);
    assert!(!desugared.declarations.is_empty());
}

#[test]
fn test_desugar_valid_06_traits() {
    let source = read_fixture("valid/06_traits.mds");
    let program = parse_program(&source).expect("Parsing failed");
    let env = check_program(&program).expect("Typecheck failed");
    let desugared = desugar_program(&program, &env);
    assert!(!desugared.declarations.is_empty());
}

#[test]
fn test_desugar_valid_07_effects() {
    let source = read_fixture("valid/07_effects.mds");
    let program = parse_program(&source).expect("Parsing failed");
    let env = check_program(&program).expect("Typecheck failed");
    let desugared = desugar_program(&program, &env);
    assert!(!desugared.declarations.is_empty());

    // Verify fetch_user_input has eliminated `check` into a match
    let fetch_fn = desugared
        .declarations
        .iter()
        .find_map(|d| match d {
            DesugaredDecl::Function(f) if f.name == "fetch_user_input" => Some(f),
            _ => None,
        })
        .expect("fetch_user_input must exist");

    // Check that there is a Match expression in the body (from check parse_int(raw))
    let has_match = fetch_fn.body.iter().any(|stmt| match stmt {
        DesugaredStmt::Let { initializer, .. } => {
            matches!(initializer.kind, DesugaredExprKind::Match { .. })
        }
        _ => false,
    });
    assert!(has_match, "check expression must be desugared into a Match");
}

#[test]
fn test_desugar_valid_08_match() {
    let source = read_fixture("valid/08_match.mds");
    let program = parse_program(&source).expect("Parsing failed");
    let env = check_program(&program).expect("Typecheck failed");
    let desugared = desugar_program(&program, &env);
    assert!(!desugared.declarations.is_empty());
}

#[test]
fn test_desugar_valid_09_closures() {
    let source = read_fixture("valid/09_closures.mds");
    let program = parse_program(&source).expect("Parsing failed");
    let env = check_program(&program).expect("Typecheck failed");
    let desugared = desugar_program(&program, &env);
    assert!(!desugared.declarations.is_empty());
}

#[test]
fn test_desugar_valid_10_arrays() {
    let source = read_fixture("valid/10_arrays.mds");
    let program = parse_program(&source).expect("Parsing failed");
    let env = check_program(&program).expect("Typecheck failed");
    let desugared = desugar_program(&program, &env);
    assert!(!desugared.declarations.is_empty());
}

#[test]
fn test_desugar_valid_11_operators() {
    let source = read_fixture("valid/11_operators.mds");
    let program = parse_program(&source).expect("Parsing failed");
    let env = check_program(&program).expect("Typecheck failed");
    let desugared = desugar_program(&program, &env);
    assert!(!desugared.declarations.is_empty());
}

#[test]
fn test_desugar_valid_12_full_program() {
    let source = read_fixture("valid/12_full_program.mds");
    let program = parse_program(&source).expect("Parsing failed");
    let env = check_program(&program).expect("Typecheck failed");
    let desugared = desugar_program(&program, &env);
    assert!(!desugared.declarations.is_empty());
}

// ============================================================================
// Unit Tests: check and perform Desugaring Ordering
// ============================================================================

#[test]
fn test_check_perform_ordering() {
    // check perform query_database(id) => run IO (perform) first, then match Result (check)
    let source = r#"
        function query(id: i32): IO(Result(String, String)) {
            return IO.pure(Result.Ok("data"));
        }

        function run(): IO(Result(String, String)) {
            let data = check perform query(42);
            return Result.Ok(data);
        }
    "#;
    let program = parse_program(source).expect("Parsing failed");
    let env = check_program(&program).expect("Typecheck failed");
    let desugared = desugar_program(&program, &env);

    let run_fn = desugared
        .declarations
        .iter()
        .find_map(|d| match d {
            DesugaredDecl::Function(f) if f.name == "run" => Some(f),
            _ => None,
        })
        .expect("run function must exist");

    // The let data statement should have initializer = Match on Perform(query(42))
    let let_data_stmt = &run_fn.body[0];
    if let DesugaredStmt::Let { initializer, .. } = let_data_stmt {
        if let DesugaredExprKind::Match { expr, arms } = &initializer.kind {
            assert!(
                matches!(
                    expr.kind,
                    DesugaredExprKind::Unary {
                        op: DesugaredUnaryOp::Perform,
                        ..
                    }
                ),
                "check perform must perform IO as the target of the match"
            );
            assert_eq!(arms.len(), 2, "Result match must have Ok and Err arms");
        } else {
            panic!("Expected Match initializer, got {:?}", initializer.kind);
        }
    } else {
        panic!("Expected Let statement");
    }
}

#[test]
fn test_perform_check_ordering() {
    // perform check res_io => match Result (check) first, then perform IO
    let source = r#"
        function run(res_io: Result(IO(String), String)): IO(Result(String, String)) {
            let val = perform check res_io;
            return Result.Ok(val);
        }
    "#;
    let program = parse_program(source).expect("Parsing failed");
    let env = check_program(&program).expect("Typecheck failed");
    let desugared = desugar_program(&program, &env);

    let run_fn = desugared
        .declarations
        .iter()
        .find_map(|d| match d {
            DesugaredDecl::Function(f) if f.name == "run" => Some(f),
            _ => None,
        })
        .expect("run function must exist");

    let let_val_stmt = &run_fn.body[0];
    if let DesugaredStmt::Let { initializer, .. } = let_val_stmt {
        if let DesugaredExprKind::Unary {
            op: DesugaredUnaryOp::Perform,
            expr,
        } = &initializer.kind
        {
            assert!(
                matches!(expr.kind, DesugaredExprKind::Match { .. }),
                "perform check must check Result first via Match before performing"
            );
        } else {
            panic!("Expected Perform unary, got {:?}", initializer.kind);
        }
    } else {
        panic!("Expected Let statement");
    }
}

// ============================================================================
// Unit Tests: Record Functional Update Desugaring
// ============================================================================

#[test]
fn test_record_functional_update_multiple_fields() {
    let source = r#"
        type Point3D = {
            x: i32,
            y: i32,
            z: i32,
        };

        function move_z(pt: Point3D, new_z: i32): Point3D {
            return { ...pt, z: new_z };
        }
    "#;
    let program = parse_program(source).expect("Parsing failed");
    let env = check_program(&program).expect("Typecheck failed");
    let desugared = desugar_program(&program, &env);

    let move_fn = desugared
        .declarations
        .iter()
        .find_map(|d| match d {
            DesugaredDecl::Function(f) if f.name == "move_z" => Some(f),
            _ => None,
        })
        .expect("move_z function must exist");

    if let Some(DesugaredStmt::Return(Some(ret_expr), _)) = move_fn.body.last() {
        if let DesugaredExprKind::Record(fields) = &ret_expr.kind {
            let names: Vec<&str> = fields.iter().map(|(n, _)| n.as_str()).collect();
            assert_eq!(names, vec!["x", "y", "z"]);
            // z should be Ident("new_z"), x and y should be FieldAccess on pt
            let z_field = fields.iter().find(|(n, _)| n == "z").unwrap();
            assert!(matches!(z_field.1.kind, DesugaredExprKind::Ident(ref id) if id == "new_z"));

            let x_field = fields.iter().find(|(n, _)| n == "x").unwrap();
            if let DesugaredExprKind::FieldAccess { receiver, field } = &x_field.1.kind {
                assert_eq!(field, "x");
                assert!(matches!(receiver.kind, DesugaredExprKind::Ident(ref id) if id == "pt"));
            } else {
                panic!("Expected field access for x");
            }
        } else {
            panic!("Expected Record literal");
        }
    } else {
        panic!("Expected Return");
    }
}
