use modus::parser::parse_program;
use modus::typechecker::{TypeErrorKind, check_program};
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
// Valid Semantic Fixtures: All 12 valid fixture programs must typecheck cleanly
// ============================================================================

#[test]
fn test_semantic_valid_01_primitives() {
    let source = read_fixture("valid/01_primitives.mds");
    let program = parse_program(&source).expect("Parsing failed for 01_primitives.mds");
    let result = check_program(&program);
    assert!(
        result.is_ok(),
        "Expected 01_primitives.mds to typecheck, got: {:?}",
        result.err()
    );
}

#[test]
fn test_semantic_valid_02_functions() {
    let source = read_fixture("valid/02_functions.mds");
    let program = parse_program(&source).expect("Parsing failed for 02_functions.mds");
    let result = check_program(&program);
    assert!(
        result.is_ok(),
        "Expected 02_functions.mds to typecheck, got: {:?}",
        result.err()
    );
}

#[test]
fn test_semantic_valid_03_control_flow() {
    let source = read_fixture("valid/03_control_flow.mds");
    let program = parse_program(&source).expect("Parsing failed for 03_control_flow.mds");
    let result = check_program(&program);
    assert!(
        result.is_ok(),
        "Expected 03_control_flow.mds to typecheck, got: {:?}",
        result.err()
    );
}

#[test]
fn test_semantic_valid_04_records() {
    let source = read_fixture("valid/04_records.mds");
    let program = parse_program(&source).expect("Parsing failed for 04_records.mds");
    let result = check_program(&program);
    assert!(
        result.is_ok(),
        "Expected 04_records.mds to typecheck, got: {:?}",
        result.err()
    );
}

#[test]
fn test_semantic_valid_05_generics() {
    let source = read_fixture("valid/05_generics.mds");
    let program = parse_program(&source).expect("Parsing failed for 05_generics.mds");
    let result = check_program(&program);
    assert!(
        result.is_ok(),
        "Expected 05_generics.mds to typecheck, got: {:?}",
        result.err()
    );
}

#[test]
fn test_semantic_valid_06_traits() {
    let source = read_fixture("valid/06_traits.mds");
    let program = parse_program(&source).expect("Parsing failed for 06_traits.mds");
    let result = check_program(&program);
    assert!(
        result.is_ok(),
        "Expected 06_traits.mds to typecheck, got: {:?}",
        result.err()
    );
}

#[test]
fn test_semantic_valid_07_effects() {
    let source = read_fixture("valid/07_effects.mds");
    let program = parse_program(&source).expect("Parsing failed for 07_effects.mds");
    let result = check_program(&program);
    assert!(
        result.is_ok(),
        "Expected 07_effects.mds to typecheck, got: {:?}",
        result.err()
    );
}

#[test]
fn test_semantic_valid_08_match() {
    let source = read_fixture("valid/08_match.mds");
    let program = parse_program(&source).expect("Parsing failed for 08_match.mds");
    let result = check_program(&program);
    assert!(
        result.is_ok(),
        "Expected 08_match.mds to typecheck, got: {:?}",
        result.err()
    );
}

#[test]
fn test_semantic_valid_09_closures() {
    let source = read_fixture("valid/09_closures.mds");
    let program = parse_program(&source).expect("Parsing failed for 09_closures.mds");
    let result = check_program(&program);
    assert!(
        result.is_ok(),
        "Expected 09_closures.mds to typecheck, got: {:?}",
        result.err()
    );
}

#[test]
fn test_semantic_valid_10_arrays() {
    let source = read_fixture("valid/10_arrays.mds");
    let program = parse_program(&source).expect("Parsing failed for 10_arrays.mds");
    let result = check_program(&program);
    assert!(
        result.is_ok(),
        "Expected 10_arrays.mds to typecheck, got: {:?}",
        result.err()
    );
}

#[test]
fn test_semantic_valid_11_operators() {
    let source = read_fixture("valid/11_operators.mds");
    let program = parse_program(&source).expect("Parsing failed for 11_operators.mds");
    let result = check_program(&program);
    assert!(
        result.is_ok(),
        "Expected 11_operators.mds to typecheck, got: {:?}",
        result.err()
    );
}

#[test]
fn test_semantic_valid_12_full_program() {
    let source = read_fixture("valid/12_full_program.mds");
    let program = parse_program(&source).expect("Parsing failed for 12_full_program.mds");
    let result = check_program(&program);
    assert!(
        result.is_ok(),
        "Expected 12_full_program.mds to typecheck, got: {:?}",
        result.err()
    );
}

// ============================================================================
// Semantic Error Fixtures: Must be rejected with specific semantic errors
// ============================================================================

#[test]
fn test_semantic_error_01_perform_in_non_io() {
    let source = read_fixture("semantic/01_perform_in_non_io.mds");
    let program =
        parse_program(&source).expect("Parsing must succeed for syntactically valid code");
    let result = check_program(&program);
    assert!(
        result.is_err(),
        "Expected purity error for perform in non-IO function"
    );
    let err = result.unwrap_err();
    assert!(
        matches!(err.kind, TypeErrorKind::PurityViolation { .. }),
        "Expected PurityViolation, got: {:?}",
        err
    );
}

#[test]
fn test_semantic_error_02_pure_function_void() {
    let source = read_fixture("semantic/02_pure_function_void.mds");
    let program = parse_program(&source).expect("Parsing must succeed");
    let result = check_program(&program);
    assert!(
        result.is_err(),
        "Expected dead computation error for pure void function"
    );
    let err = result.unwrap_err();
    assert!(
        matches!(err.kind, TypeErrorKind::DeadComputation { .. }),
        "Expected DeadComputation, got: {:?}",
        err
    );
}

#[test]
fn test_semantic_error_03_record_type_mismatch() {
    let source = read_fixture("semantic/03_record_type_mismatch.mds");
    let program = parse_program(&source).expect("Parsing must succeed");
    let result = check_program(&program);
    assert!(
        result.is_err(),
        "Expected type mismatch error for record literal field"
    );
    let err = result.unwrap_err();
    assert!(
        matches!(err.kind, TypeErrorKind::TypeMismatch { .. }),
        "Expected TypeMismatch, got: {:?}",
        err
    );
}

#[test]
fn test_semantic_error_04_unimplemented_trait() {
    let source = read_fixture("semantic/04_unimplemented_trait.mds");
    let program = parse_program(&source).expect("Parsing must succeed");
    let result = check_program(&program);
    assert!(
        result.is_err(),
        "Expected trait resolution error for unimplemented trait"
    );
    let err = result.unwrap_err();
    assert!(
        matches!(
            err.kind,
            TypeErrorKind::TraitNotImplemented { .. } | TypeErrorKind::TypeMismatch { .. }
        ),
        "Expected TraitNotImplemented or TypeMismatch, got: {:?}",
        err
    );
}

#[test]
fn test_semantic_error_05_check_non_result() {
    let source = read_fixture("semantic/05_check_non_result.mds");
    let program = parse_program(&source).expect("Parsing must succeed");
    let result = check_program(&program);
    assert!(result.is_err(), "Expected check error on non-Result type");
    let err = result.unwrap_err();
    assert!(
        matches!(err.kind, TypeErrorKind::CheckOnNonResult { .. }),
        "Expected CheckOnNonResult, got: {:?}",
        err
    );
}

#[test]
fn test_semantic_error_06_undeclared_variable() {
    let source = read_fixture("semantic/06_undeclared_variable.mds");
    let program = parse_program(&source).expect("Parsing must succeed");
    let result = check_program(&program);
    assert!(result.is_err(), "Expected error for undeclared variable");
    let err = result.unwrap_err();
    assert!(
        matches!(err.kind, TypeErrorKind::UndeclaredVariable(..)),
        "Expected UndeclaredVariable, got: {:?}",
        err
    );
}

#[test]
fn test_semantic_error_07_duplicate_variable() {
    let source = read_fixture("semantic/07_duplicate_variable.mds");
    let program = parse_program(&source).expect("Parsing must succeed");
    let result = check_program(&program);
    assert!(
        result.is_err(),
        "Expected error for duplicate variable in same scope"
    );
    let err = result.unwrap_err();
    assert!(
        matches!(err.kind, TypeErrorKind::DuplicateVariable(..)),
        "Expected DuplicateVariable, got: {:?}",
        err
    );
}

// ============================================================================
// Unit Tests: Scope & Symbol Table
// ============================================================================

#[test]
fn test_scope_nested_lookups_and_duplicate_prevention() {
    use modus::ast::Span;
    use modus::typechecker::{Environment, Type};

    let mut env = Environment::new();
    let span = Span::new(0, 10);

    // Define in root scope
    env.define_var("x".to_string(), Type::i32(), span)
        .expect("Should define x");
    assert_eq!(env.lookup_var("x").unwrap().0, Type::i32());

    // Duplicate in same scope must fail
    let err = env
        .define_var("x".to_string(), Type::i32(), span)
        .unwrap_err();
    assert!(matches!(err.kind, TypeErrorKind::DuplicateVariable(ref name) if name == "x"));

    // Enter nested scope
    env.enter_scope();
    // Shadowing or inner variable lookup
    env.define_var("y".to_string(), Type::string(), span)
        .expect("Should define y");
    assert_eq!(env.lookup_var("y").unwrap().0, Type::string());
    assert_eq!(env.lookup_var("x").unwrap().0, Type::i32()); // Lookup parent scope

    // Exit nested scope
    env.exit_scope();
    assert!(env.lookup_var("y").is_none());
    assert_eq!(env.lookup_var("x").unwrap().0, Type::i32());
}

// ============================================================================
// Unit Tests: Types & Unification
// ============================================================================

#[test]
fn test_type_structural_record_unification() {
    use modus::ast::Span;
    use modus::typechecker::{Substitution, Type};
    use std::collections::BTreeMap;

    let mut r1_fields = BTreeMap::new();
    r1_fields.insert("x".to_string(), Type::i32());
    r1_fields.insert("y".to_string(), Type::i32());
    let r1 = Type::Record(r1_fields);

    let mut r2_fields = BTreeMap::new();
    r2_fields.insert("y".to_string(), Type::i32());
    r2_fields.insert("x".to_string(), Type::i32());
    let r2 = Type::Record(r2_fields);

    // Structural equality: field order does not matter
    assert_eq!(r1, r2);

    let mut subst = Substitution::new();
    let result = subst.unify(&r1, &r2, Some(Span::new(0, 5)), &|_, _| None);
    assert!(result.is_ok(), "Structural records must unify");
}

#[test]
fn test_type_variable_unification_and_occurs_check() {
    use modus::ast::Span;
    use modus::typechecker::{Substitution, Type, TypeVarGen};

    let mut var_gen = TypeVarGen::new();
    let v0 = var_gen.fresh();
    let v1 = var_gen.fresh();

    let mut subst = Substitution::new();
    let span = Some(Span::new(0, 5));

    // Unify ?T0 with i32
    assert!(subst.unify(&v0, &Type::i32(), span, &|_, _| None).is_ok());
    assert_eq!(subst.apply(&v0), Type::i32());

    // Unify ?T1 with [?T0] -> [i32]
    let arr = Type::Array(Box::new(v0.clone()));
    assert!(subst.unify(&v1, &arr, span, &|_, _| None).is_ok());
    assert_eq!(subst.apply(&v1), Type::Array(Box::new(Type::i32())));

    // Occurs check: unify ?T2 with [?T2] -> should fail
    let v2 = var_gen.fresh();
    let recursive_arr = Type::Array(Box::new(v2.clone()));
    let err = subst
        .unify(&v2, &recursive_arr, span, &|_, _| None)
        .unwrap_err();
    assert_eq!(err.kind, TypeErrorKind::OccursCheckFailed);
}

// ============================================================================
// Unit Tests: Bidirectional Inference
// ============================================================================

#[test]
fn test_bidirectional_record_missing_field() {
    let source = r#"
        type Config = {
            host: String,
            port: u16,
        };

        function make_cfg(): Config {
            return { host: "localhost" };
        }
    "#;
    let program = parse_program(source).expect("Parsing must succeed");
    let result = check_program(&program);
    assert!(result.is_err(), "Expected error for missing record field");
    let err = result.unwrap_err();
    assert!(
        matches!(err.kind, TypeErrorKind::MissingRecordField { ref field, .. } if field == "port"),
        "Expected MissingRecordField 'port', got: {:?}",
        err
    );
}

#[test]
fn test_bidirectional_record_extraneous_field() {
    let source = r#"
        type Config = {
            port: u16,
        };

        function make_cfg(): Config {
            return { port: 8080, extra: true };
        }
    "#;
    let program = parse_program(source).expect("Parsing must succeed");
    let result = check_program(&program);
    assert!(
        result.is_err(),
        "Expected error for extraneous record field"
    );
    let err = result.unwrap_err();
    assert!(
        matches!(err.kind, TypeErrorKind::ExtraneousRecordField { ref field, .. } if field == "extra"),
        "Expected ExtraneousRecordField 'extra', got: {:?}",
        err
    );
}

// ============================================================================
// Unit Tests: Trait Resolution & Bounds
// ============================================================================

#[test]
fn test_trait_missing_method_in_impl() {
    let source = r#"
        trait Greeter(Self) {
            function greet(self: Self): String;
            function farewell(self: Self): String;
        }

        type Person = { name: String };

        impl Greeter for Person {
            function greet(self: Person): String => "Hello";
        }
    "#;
    let program = parse_program(source).expect("Parsing must succeed");
    let result = check_program(&program);
    assert!(result.is_err(), "Expected error for missing method in impl");
    let err = result.unwrap_err();
    assert!(
        matches!(err.kind, TypeErrorKind::TraitNotImplemented { .. }),
        "Expected TraitNotImplemented, got: {:?}",
        err
    );
}

#[test]
fn test_trait_method_signature_mismatch_in_impl() {
    let source = r#"
        trait Greeter(Self) {
            function greet(self: Self): String;
        }

        type Person = { name: String };

        impl Greeter for Person {
            function greet(self: Person): i32 => 42;
        }
    "#;
    let program = parse_program(source).expect("Parsing must succeed");
    let result = check_program(&program);
    assert!(
        result.is_err(),
        "Expected error for method signature return type mismatch in impl"
    );
    let err = result.unwrap_err();
    assert!(
        matches!(err.kind, TypeErrorKind::TypeMismatch { .. }),
        "Expected TypeMismatch, got: {:?}",
        err
    );
}

#[test]
fn test_ffi_effectful_extern_in_pure_rejected() {
    let source = r#"
extern "C" {
    function puts(s: CString): IO(i32);
}

function impure_caller(): i32 {
    let s: CString = String.toCString("test");
    return puts(s);
}
"#;
    let program = parse_program(source).expect("Parsing must succeed");
    let result = check_program(&program);
    assert!(
        result.is_err(),
        "Calling effectful extern function in pure function must fail"
    );
    let err = result.unwrap_err();
    assert!(
        matches!(err.kind, TypeErrorKind::PurityViolation { .. }),
        "Expected PurityViolation, got: {:?}",
        err
    );
}

#[test]
fn test_ffi_pure_extern_rejected() {
    let source = r#"
extern "C" {
    function strlen(s: CString): u64;
}
"#;
    let program = parse_program(source).expect("Parsing must succeed");
    let result = check_program(&program);
    assert!(
        result.is_err(),
        "Pure extern function must be rejected: all extern functions must return IO"
    );
    let err = result.unwrap_err();
    assert!(
        matches!(err.kind, TypeErrorKind::ExternFunctionMustReturnIO { .. }),
        "Expected ExternFunctionMustReturnIO, got: {:?}",
        err
    );
}

#[test]
fn test_ffi_pure_extern_void_rejected() {
    let source = r#"
extern "C" {
    function abort(): void;
}
"#;
    let program = parse_program(source).expect("Parsing must succeed");
    let result = check_program(&program);
    assert!(
        result.is_err(),
        "Void extern function must be rejected without IO: all extern functions must return IO"
    );
    let err = result.unwrap_err();
    assert!(
        matches!(err.kind, TypeErrorKind::ExternFunctionMustReturnIO { .. }),
        "Expected ExternFunctionMustReturnIO, got: {:?}",
        err
    );
}

#[test]
fn test_pointer_operations_typecheck() {
    let source = r#"
extern "C" {
    function malloc(size: u64): IO(Pointer(void));
    function free(ptr: Pointer(void)): IO(void);
}

function test_ptrs(): IO(i32) {
    let raw: Pointer(void) = perform malloc(8);
    let p: Pointer(i32) = raw.cast();
    perform p.write(42);
    let p_next: Pointer(i32) = p.offset(1);
    perform p_next.write(100);
    let val: i32 = perform p.read();
    let addr: u64 = p.address();
    let is_null: bool = p.isNull();
    let null_ptr: Pointer(i32) = Pointer.null();
    perform free(p.cast());
    return IO.pure(val);
}
"#;
    let program = parse_program(source).expect("Parsing must succeed");
    let result = check_program(&program);
    assert!(
        result.is_ok(),
        "Pointer operations must typecheck cleanly: {:?}",
        result.err()
    );
}

#[test]
fn test_extern_with_body_rejected() {
    let source = r#"
extern "C" {
    function puts(s: CString): IO(i32) {
        return IO.pure(0);
    }
}
"#;
    let program = parse_program(source).expect("Parsing must succeed");
    let result = check_program(&program);
    assert!(
        result.is_err(),
        "Extern function with body must be rejected"
    );
}

#[test]
fn test_typecheck_valid_casts() {
    let source = r#"
function test_casts(p: Pointer(i32)): IO(void) {
    let a: i64 = 42 as i64;
    let b: u8 = a as u8;
    let c: i32 = b as i32;
    let f: f32 = 3.14 as f32;
    let d: f64 = f as f64;
    let fi: f64 = 42 as f64;
    let ifl: i32 = fi as i32;
    let bi: i32 = true as i32;
    let ib: bool = 1 as bool;
    let addr: u64 = p as u64;
    let p2: Pointer(i32) = addr as Pointer(i32);
    let p_void: Pointer(void) = p as Pointer(void);
    return IO.pure(());
}
"#;
    let program = parse_program(source).expect("Parsing must succeed");
    let result = check_program(&program);
    assert!(
        result.is_ok(),
        "Valid casts should typecheck: {:?}",
        result.err()
    );
}

#[test]
fn test_typecheck_invalid_casts() {
    let bad_str = r#"
function f(): i32 {
    return "hello" as i32;
}
"#;
    let prog1 = parse_program(bad_str).unwrap();
    let res1 = check_program(&prog1);
    assert!(res1.is_err());
    assert!(matches!(
        res1.unwrap_err().kind,
        TypeErrorKind::InvalidCast { .. }
    ));

    let bad_arr = r#"
function f(): f64 {
    return [1, 2, 3] as f64;
}
"#;
    let prog2 = parse_program(bad_arr).unwrap();
    let res2 = check_program(&prog2);
    assert!(res2.is_err());
    assert!(matches!(
        res2.unwrap_err().kind,
        TypeErrorKind::InvalidCast { .. }
    ));
}

#[test]
fn test_extern_foreign_symbol_alias() {
    let src = r#"
extern "C" {
    function c_rename(oldpath: CString, newpath: CString): IO(i32) = "rename";
}

function my_rename(old: String, new: String): IO(i32) {
    let res: i32 = perform c_rename(String.toCString(old), String.toCString(new));
    return IO.pure(res);
}
"#;
    let prog = parse_program(src).unwrap();
    let res = check_program(&prog);
    assert!(
        res.is_ok(),
        "Expected extern symbol alias to typecheck: {:?}",
        res.err()
    );
}

#[test]
fn test_normal_function_with_alias_no_body_rejected() {
    let src = r#"
function foo(x: i32): i32 = "c_foo";
"#;
    let prog = parse_program(src).unwrap();
    let res = check_program(&prog);
    assert!(res.is_err());
    assert!(matches!(
        res.unwrap_err().kind,
        TypeErrorKind::MissingFunctionBody { .. }
    ));
}
