use modus::desugar::desugar_program;
use modus::ir::*;
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

fn compile_to_anf(source: &str) -> AnfProgram {
    let program = parse_program(source).expect("Parsing failed");
    let env = check_program(&program).expect("Typecheck failed");
    let desugared = desugar_program(&program, &env);
    lower_program(&desugared)
}

// ============================================================================
// 1. All 12 Valid Fixtures Lower to ANF cleanly
// ============================================================================

#[test]
fn test_anf_lower_valid_01_primitives() {
    let source = read_fixture("valid/01_primitives.mds");
    let anf = compile_to_anf(&source);
    assert!(!anf.functions.is_empty());
    let test_fn = anf
        .functions
        .iter()
        .find(|f| f.name == "test_primitives")
        .expect("test_primitives must exist");
    assert!(matches!(test_fn.body.tail, AnfTail::Return(Some(_))));
}

#[test]
fn test_anf_lower_valid_02_functions() {
    let source = read_fixture("valid/02_functions.mds");
    let anf = compile_to_anf(&source);
    assert!(!anf.functions.is_empty());
    // Square function was lowered to ANF
    let sq_fn = anf
        .functions
        .iter()
        .find(|f| f.name == "square")
        .expect("square must exist");
    assert!(matches!(sq_fn.body.tail, AnfTail::Return(Some(_))));
}

#[test]
fn test_anf_lower_valid_03_control_flow() {
    let source = read_fixture("valid/03_control_flow.mds");
    let anf = compile_to_anf(&source);
    assert!(!anf.functions.is_empty());
}

#[test]
fn test_anf_lower_valid_04_records() {
    let source = read_fixture("valid/04_records.mds");
    let anf = compile_to_anf(&source);
    assert!(!anf.functions.is_empty());
}

#[test]
fn test_anf_lower_valid_05_generics() {
    let source = read_fixture("valid/05_generics.mds");
    let anf = compile_to_anf(&source);
    assert!(!anf.functions.is_empty());
}

#[test]
fn test_anf_lower_valid_06_traits() {
    let source = read_fixture("valid/06_traits.mds");
    let anf = compile_to_anf(&source);
    assert!(!anf.traits.is_empty());
    assert!(!anf.impls.is_empty());
}

#[test]
fn test_anf_lower_valid_07_effects() {
    let source = read_fixture("valid/07_effects.mds");
    let anf = compile_to_anf(&source);
    assert!(!anf.functions.is_empty());
}

#[test]
fn test_anf_lower_valid_08_match() {
    let source = read_fixture("valid/08_match.mds");
    let anf = compile_to_anf(&source);
    assert!(!anf.functions.is_empty());
}

#[test]
fn test_anf_lower_valid_09_closures() {
    let source = read_fixture("valid/09_closures.mds");
    let anf = compile_to_anf(&source);
    assert!(!anf.functions.is_empty());
}

#[test]
fn test_anf_lower_valid_10_arrays() {
    let source = read_fixture("valid/10_arrays.mds");
    let anf = compile_to_anf(&source);
    assert!(!anf.functions.is_empty());
}

#[test]
fn test_anf_lower_valid_11_operators() {
    let source = read_fixture("valid/11_operators.mds");
    let anf = compile_to_anf(&source);
    assert!(!anf.functions.is_empty());
}

#[test]
fn test_anf_lower_valid_12_full_program() {
    let source = read_fixture("valid/12_full_program.mds");
    let mut anf = compile_to_anf(&source);
    assert!(!anf.functions.is_empty());
    assert!(!anf.impls.is_empty());

    // Run full pipeline on full program: closure conversion, liveness, perceus & fbip
    convert_closures(&mut anf);
    apply_perceus_and_fbip(&mut anf);
    assert!(!anf.functions.is_empty());
}

// ============================================================================
// 2. Linearization & ANF Invariants
// ============================================================================

#[test]
fn test_anf_expression_linearization() {
    let source = r#"
        function compute(a: i32, b: i32, c: i32): i32 {
            let res = (a + b) * (b - c);
            return res;
        }
    "#;
    let anf = compile_to_anf(source);
    let func = anf
        .functions
        .iter()
        .find(|f| f.name == "compute")
        .expect("compute function");

    // Check that sub-expressions are bound to temporaries
    // Should have: let _t0 = a + b; let _t1 = b - c; let res = _t0 * _t1;
    let has_binary_stmts = func
        .body
        .stmts
        .iter()
        .filter(|s| {
            matches!(
                s,
                AnfStmt::Let {
                    value: AnfExpr::Binary { .. },
                    ..
                }
            )
        })
        .count();

    assert!(
        has_binary_stmts >= 3,
        "Expected at least 3 linearized binary operations"
    );

    // Verify all operands to binary operations are atoms
    for stmt in &func.body.stmts {
        if let AnfStmt::Let {
            value: AnfExpr::Binary { lhs, rhs, .. },
            ..
        } = stmt
        {
            assert!(lhs.is_var() || matches!(lhs, Atom::Literal(_)));
            assert!(rhs.is_var() || matches!(rhs, Atom::Literal(_)));
        }
    }
}

// ============================================================================
// 3. Direct Recursion Tail Call Detection (`musttail`)
// ============================================================================

#[test]
fn test_anf_tail_call_detection() {
    let source = r#"
        function gcd(a: i32, b: i32): i32 {
            if (b == 0) {
                return a;
            } else {
                return gcd(b, a % b);
            }
        }
    "#;
    let anf = compile_to_anf(source);
    let func = anf
        .functions
        .iter()
        .find(|f| f.name == "gcd")
        .expect("gcd function");

    // The tail of the function should be an If whose else_branch terminates in a TailCall
    if let AnfTail::If { else_branch, .. } = &func.body.tail {
        let eb = else_branch.as_ref().expect("else_branch must exist");
        assert!(
            matches!(&eb.tail, AnfTail::TailCall { callee: Atom::Var(name), args } if name == "gcd" && args.len() == 2),
            "Expected tail call to gcd in else branch, found: {:?}",
            eb.tail
        );
    } else {
        panic!(
            "Expected AnfTail::If for gcd body, found: {:?}",
            func.body.tail
        );
    }
}

// ============================================================================
// 4. Closure Conversion & Lambda Lifting
// ============================================================================

#[test]
fn test_closure_conversion_capturing() {
    let source = r#"
        function make_adder(x: i32): (i32) => i32 {
            return (y: i32) => x + y;
        }
    "#;
    let mut anf = compile_to_anf(source);
    convert_closures(&mut anf);

    // Closure conversion should have:
    // 1. Created an environment struct type: _Env__lambda_0 with field x: i32
    let env_type = anf
        .types
        .iter()
        .find(|t| t.name.starts_with("_Env_"))
        .expect("Environment struct type should be created");
    assert!(env_type.name.contains("_lambda_"));

    // 2. Created a lifted top-level function: _lambda_0
    let lifted_fn = anf
        .functions
        .iter()
        .find(|f| f.name.starts_with("_lambda_"))
        .expect("Lifted lambda function should be created");

    // 3. Lifted function takes `_env` as its first parameter and `y` as second
    assert_eq!(lifted_fn.params.len(), 2);
    assert_eq!(lifted_fn.params[0].0, "_env");
    assert_eq!(lifted_fn.params[1].0, "y");

    // 4. Inside the lifted function, `x` is unpacked from `_env.x`
    let has_env_unpack = lifted_fn.body.stmts.iter().any(|s| {
        matches!(
            s,
            AnfStmt::Let {
                var,
                value: AnfExpr::FieldAccess { receiver: Atom::Var(env), field },
                ..
            } if var == "x" && env == "_env" && field == "x"
        )
    });
    assert!(
        has_env_unpack,
        "Lambda must unpack captured variable x from _env"
    );

    // 5. In make_adder, environment record is allocated and MakeClosure is called
    let adder_fn = anf
        .functions
        .iter()
        .find(|f| f.name == "make_adder")
        .expect("make_adder function");
    let has_make_closure = adder_fn.body.stmts.iter().any(|s| {
        matches!(
            s,
            AnfStmt::Let {
                value: AnfExpr::MakeClosure { fn_name, env: Some(_) },
                ..
            } if fn_name.starts_with("_lambda_")
        )
    });
    assert!(
        has_make_closure,
        "make_adder must construct environment and instantiate closure"
    );
}

// ============================================================================
// 5. Liveness and Borrow Analysis
// ============================================================================

#[test]
fn test_liveness_last_use_and_shared_use() {
    let source = r#"
        type Point = { x: i32, y: i32 };

        function test_liveness(pt: Point): i32 {
            let a = pt.x;
            let b = pt.y;
            return a + b;
        }
    "#;
    let anf = compile_to_anf(source);
    let func = anf
        .functions
        .iter()
        .find(|f| f.name == "test_liveness")
        .expect("test_liveness");

    let liveness = analyze_function_liveness(func);

    // `pt` is used in statement 0 (`pt.x`) and statement 1 (`pt.y`).
    // In statement 0: `pt` is NOT at its last use (pt is in live_out).
    let stmt0 = &liveness.block.stmts[0];
    assert!(
        stmt0.live_out.contains("pt"),
        "pt must be live after first use in pt.x"
    );
    assert!(
        !stmt0.last_uses.contains("pt"),
        "statement 0 is not the last use of pt"
    );

    // In statement 1: `pt` IS at its last use (`pt.y`), pt is NOT in live_out.
    let stmt1 = &liveness.block.stmts[1];
    assert!(
        !stmt1.live_out.contains("pt"),
        "pt must NOT be live after pt.y"
    );
    assert!(
        stmt1.last_uses.contains("pt"),
        "statement 1 must be the last use of pt"
    );
}

// ============================================================================
// 6. Perceus Reference Counting (inc_ref / dec_ref)
// ============================================================================

#[test]
fn test_perceus_unboxed_primitives_no_rc() {
    let source = r#"
        function int_math(x: i32, y: i32): i32 {
            let sum = x + y;
            let prod = x * sum;
            return prod;
        }
    "#;
    let mut anf = compile_to_anf(source);
    apply_perceus_and_fbip(&mut anf);

    let func = anf
        .functions
        .iter()
        .find(|f| f.name == "int_math")
        .expect("int_math");

    // Unboxed primitives (i32) must NOT have any IncRef or DecRef operations!
    let rc_ops_count = func
        .body
        .stmts
        .iter()
        .filter(|s| matches!(s, AnfStmt::IncRef { .. } | AnfStmt::DecRef { .. }))
        .count();

    assert_eq!(
        rc_ops_count, 0,
        "Primitives must be unboxed with 0 RC operations"
    );
}

#[test]
fn test_perceus_shared_heap_object_inc_ref() {
    let source = r#"
        type Point = { x: i32, y: i32 };
        type Pair = { first: Point, second: Point };

        function share_record(pt: Point): Pair {
            return { first: pt, second: pt };
        }
    "#;
    let mut anf = compile_to_anf(source);
    apply_perceus_and_fbip(&mut anf);

    let func = anf
        .functions
        .iter()
        .find(|f| f.name == "share_record")
        .expect("share_record");

    // `pt` is a heap object used multiple times in the record. IncRef must be inserted for shared use!
    let has_inc_ref = func
        .body
        .stmts
        .iter()
        .any(|s| matches!(s, AnfStmt::IncRef { var } if var == "pt"));

    assert!(
        has_inc_ref,
        "Shared heap record must have inc_ref inserted: {:?}",
        func.body.stmts
    );
}

// ============================================================================
// 7. FBIP (Functional But In-Place) Optimization
// ============================================================================

#[test]
fn test_fbip_record_update_reuse() {
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
    let mut anf = compile_to_anf(source);
    apply_perceus_and_fbip(&mut anf);

    let func = anf
        .functions
        .iter()
        .find(|f| f.name == "move_z")
        .expect("move_z");

    // FBIP must detect that pt is at its last use and optimize { ...pt, z: new_z } into ReuseRecord
    let has_reuse = func.body.stmts.iter().any(|s| {
        matches!(
            s,
            AnfStmt::Let {
                value: AnfExpr::ReuseRecord { base: Atom::Var(b), fields },
                ..
            } if b == "pt" && fields.iter().any(|(n, _)| n == "z")
        )
    });

    assert!(
        has_reuse,
        "FBIP must optimize record functional update on last-use buffer into ReuseRecord: {:?}",
        func.body.stmts
    );
}
