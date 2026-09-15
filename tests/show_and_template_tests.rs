use inkwell::context::Context;
use modus::backend::CodeGen;
use modus::parser::{parse_expr, parse_program};
use modus::typechecker::{
    Type, check_program,
    infer::TypeInferrer,
    scope::{Environment, FunctionSig, ImplDef},
};

fn compile_to_llvm<'ctx>(
    context: &'ctx Context,
    source: &str,
    module_name: &str,
) -> Result<CodeGen<'ctx>, String> {
    // Prelude-aware single-module pipeline: `==`/`+` on `String` lower
    // through the `Eq`/`Add` impls, so the bare typecheck+desugar pipeline
    // cannot be used for programs with string operators.
    modus::compile_source(context, source, module_name)
}

/// Bare `Environment` plus the `Add` impl for `String` (what the prelude
/// provides in real builds), for operator typechecking unit tests.
fn env_with_string_add() -> Environment {
    let mut env = Environment::new();
    env.register_impl(ImplDef {
        trait_name: "Add".to_string(),
        target_type: Type::string(),
        methods: [(
            "add".to_string(),
            FunctionSig {
                name: "add".to_string(),
                type_params: vec![],
                params: vec![
                    ("self".to_string(), Type::string()),
                    ("other".to_string(), Type::string()),
                ],
                return_type: Type::string(),
                is_effectful: false,
                span: modus::ast::Span::default(),
                symbol_name: None,
                is_c_abi: false,
            },
        )]
        .into_iter()
        .collect(),
    });
    env
}

fn jit_eval_string(source: &str, module_name: &str) -> String {
    inkwell::targets::Target::initialize_native(&inkwell::targets::InitializationConfig::default())
        .expect("Failed to init native target");
    let context = Context::create();
    let codegen = compile_to_llvm(&context, source, module_name).expect("Codegen failed");
    codegen.optimize(None).expect("Optimization failed");
    let execution_engine = codegen
        .module
        .create_jit_execution_engine(inkwell::OptimizationLevel::Aggressive)
        .expect("Failed to create JIT engine");

    unsafe {
        let main_fn = execution_engine
            .get_function::<unsafe extern "C" fn() -> *const u8>("main")
            .expect("Failed to find main function");
        let ptr = main_fn.call();
        assert!(!ptr.is_null(), "Returned string pointer was null");
        let len = *(ptr.add(8) as *const i64);
        let data_ptr = ptr.add(24);
        let slice = std::slice::from_raw_parts(data_ptr, len as usize);
        std::str::from_utf8(slice).unwrap().to_string()
    }
}

// ============================================================================
// 1. Parser and Desugaring Tests for Template Strings
// ============================================================================

#[test]
fn test_parse_empty_template_string() {
    let expr = parse_expr("``").expect("Empty template string should parse");
    assert!(
        matches!(expr.node, modus::ast::Expr::Literal(modus::ast::Literal::String(ref s)) if s.is_empty())
    );
}

#[test]
fn test_parse_plain_template_string() {
    let expr = parse_expr("`hello world`").expect("Plain template string should parse");
    assert!(
        matches!(expr.node, modus::ast::Expr::Literal(modus::ast::Literal::String(ref s)) if s == "hello world")
    );
}

#[test]
fn test_parse_single_hole_template_string() {
    let expr = parse_expr("`${name}`").expect("Single hole template string should parse");
    // Desugars into (name).show()
    assert!(
        matches!(expr.node, modus::ast::Expr::MethodCall { ref method, .. } if method == "show")
    );
}

#[test]
fn test_parse_template_string_with_prefix_and_suffix() {
    let expr = parse_expr("`Hello, ${name}!`").expect("Template string should parse");
    // Desugars into ("Hello, " + (name).show()) + "!"
    assert!(matches!(expr.node, modus::ast::Expr::Binary { .. }));
}

#[test]
fn test_parse_template_string_escaped_dollar_brace() {
    let expr = parse_expr(r#"`Cost: \${100}`"#).expect("Escaped hole should parse");
    // \${100} must NOT be interpolated: produces literal "Cost: ${100}"
    assert!(
        matches!(expr.node, modus::ast::Expr::Literal(modus::ast::Literal::String(ref s)) if s == "Cost: ${100}")
    );
}

#[test]
fn test_parse_template_string_complex_expression() {
    let expr = parse_expr("`Sum: ${2 + 3 * 4}`").expect("Arithmetic in hole should parse");
    assert!(matches!(expr.node, modus::ast::Expr::Binary { .. }));
}

#[test]
fn test_parse_template_string_with_nested_braces() {
    let expr = parse_expr(r#"`Status: ${if (x > 0) { "pos" } else { "non-pos" }}`"#)
        .expect("If-else block in hole should parse");
    assert!(matches!(expr.node, modus::ast::Expr::Binary { .. }));
}

// ============================================================================
// 2. Typechecker Tests
// ============================================================================

#[test]
fn test_typecheck_primitive_show() {
    let mut env = Environment::new();
    let mut inferrer = TypeInferrer::new(&mut env, None);

    let expr = parse_expr("42.show()").unwrap();
    let ty = inferrer.synth_expr(&expr).unwrap();
    assert_eq!(ty, modus::typechecker::Type::string());

    let expr = parse_expr("true.show()").unwrap();
    let ty = inferrer.synth_expr(&expr).unwrap();
    assert_eq!(ty, modus::typechecker::Type::string());

    let expr = parse_expr(r#""test".show()"#).unwrap();
    let ty = inferrer.synth_expr(&expr).unwrap();
    assert_eq!(ty, modus::typechecker::Type::string());
}

#[test]
fn test_typecheck_string_concatenation() {
    // Bare environments have no prelude impls; register the `Add` impl for
    // `String` explicitly (graph/`compile_source` builds get it automatically).
    let mut env = env_with_string_add();
    let mut inferrer = TypeInferrer::new(&mut env, None);

    let expr = parse_expr(r#""hello " + "world""#).unwrap();
    let ty = inferrer.synth_expr(&expr).unwrap();
    assert_eq!(ty, modus::typechecker::Type::string());

    // String subtraction is forbidden
    let expr_sub = parse_expr(r#""hello" - "world""#).unwrap();
    assert!(inferrer.synth_expr(&expr_sub).is_err());
}

#[test]
fn test_typecheck_template_string() {
    let source = r#"
        function formatUser(name: String, age: i32): String {
            return `User: ${name}, Age: ${age}`;
        }
    "#;
    let program = parse_program(source).unwrap();
    // Templates fold holes with `String +`, which needs the `Add` impl.
    let mut env = env_with_string_add();
    assert!(modus::typechecker::check_program_with_env(&program, &mut env).is_ok());
}

#[test]
fn test_typecheck_custom_type_show_implementation() {
    let source = r#"
        type Point = {
            x: i32,
            y: i32,
        };

        impl Show for Point {
            function show(self: Point): String {
                return `Point(${self.x}, ${self.y})`;
            }
        }

        function printPoint(p: Point): String {
            return `Location: ${p}`;
        }
    "#;
    let program = parse_program(source).unwrap();
    // Templates fold holes with `String +`, which needs the `Add` impl.
    let mut env = env_with_string_add();
    assert!(modus::typechecker::check_program_with_env(&program, &mut env).is_ok());
}

#[test]
fn test_typecheck_unimplemented_show_rejected() {
    let source = r#"
        type Unprintable = {
            data: i32,
        };

        function test(u: Unprintable): String {
            return `Data: ${u}`;
        }
    "#;
    let program = parse_program(source).unwrap();
    assert!(
        check_program(&program).is_err(),
        "Expected typecheck failure when interpolating type without Show"
    );
}

// ============================================================================
// 3. JIT Execution Tests
// ============================================================================

#[test]
fn test_jit_primitive_show_integers() {
    let source = r#"
        function main(): String {
            let a: i32 = 42;
            let b: i64 = -12345;
            return `a = ${a}, b = ${b}`;
        }
    "#;
    let res = jit_eval_string(source, "test_jit_integers");
    assert_eq!(res, "a = 42, b = -12345");
}

#[test]
fn test_jit_primitive_show_unsigned() {
    let source = r#"
        function main(): String {
            let u: u64 = 99999999;
            return `unsigned: ${u}`;
        }
    "#;
    let res = jit_eval_string(source, "test_jit_unsigned");
    assert_eq!(res, "unsigned: 99999999");
}

#[test]
fn test_jit_primitive_show_bool() {
    let source = r#"
        function main(): String {
            let t = true;
            let f = false;
            return `${t} and ${f}`;
        }
    "#;
    let res = jit_eval_string(source, "test_jit_bool");
    assert_eq!(res, "true and false");
}

#[test]
fn test_jit_primitive_show_float() {
    let source = r#"
        function main(): String {
            let pi: f64 = 3.14159;
            return `pi: ${pi}`;
        }
    "#;
    let res = jit_eval_string(source, "test_jit_float");
    assert!(res.starts_with("pi: 3.14159"));
}

#[test]
fn test_jit_string_concat_operator() {
    let source = r#"
        function main(): String {
            let s1 = "Hello, ";
            let s2 = "Modus ";
            let s3 = "World!";
            return s1 + s2 + s3;
        }
    "#;
    let res = jit_eval_string(source, "test_jit_concat");
    assert_eq!(res, "Hello, Modus World!");
}

#[test]
fn test_jit_template_string_expressions() {
    let source = r#"
        function add(x: i32, y: i32): i32 {
            return x + y;
        }

        function main(): String {
            let a = 15;
            let b = 27;
            return `${a} + ${b} = ${add(a, b)}`;
        }
    "#;
    let res = jit_eval_string(source, "test_jit_template_expr");
    assert_eq!(res, "15 + 27 = 42");
}

#[test]
fn test_jit_template_string_escaped_dollar() {
    let source = r#"
        function main(): String {
            let price = 49;
            return `Discounted price: \${price} is ${price}`;
        }
    "#;
    let res = jit_eval_string(source, "test_jit_template_escape");
    assert_eq!(res, "Discounted price: ${price} is 49");
}

#[test]
fn test_jit_custom_impl_show() {
    let source = r#"
        type Point = {
            x: i32,
            y: i32,
        };

        impl Show for Point {
            function show(self: Point): String {
                return `(${self.x}, ${self.y})`;
            }
        }

        function main(): String {
            let p: Point = { x: 10, y: 25 };
            return `Point coordinates: ${p}`;
        }
    "#;
    let res = jit_eval_string(source, "test_jit_custom_show");
    assert_eq!(res, "Point coordinates: (10, 25)");
}

#[test]
fn test_jit_custom_impl_show_person() {
    let source = r#"
        type Person = {
            name: String,
            score: i32,
        };

        impl Show for Person {
            function show(self: Person): String {
                return `${self.name} (score: ${self.score})`;
            }
        }

        function main(): String {
            let hero: Person = { name: "Agent", score: 9001 };
            return `Hero summary: ${hero}`;
        }
    "#;
    let res = jit_eval_string(source, "test_jit_person");
    assert_eq!(res, "Hero summary: Agent (score: 9001)");
}

#[test]
fn test_template_string_with_stdio_print() {
    use modus::modules::{ModuleGraph, jit_run_graph};

    let app_src = r#"
        import { println } from "std:io";

        type Person = {
            name: String,
            score: i32,
        };

        impl Show for Person {
            function show(self: Person): String {
                return `${self.name} (score: ${self.score})`;
            }
        }

        function main(): IO(void) {
            let hero: Person = { name: "Agent", score: 9001 };
            perform println(`Hero summary: ${hero}`);
            return IO.pure(());
        }
    "#;

    let graph = ModuleGraph::build_from_source(std::path::Path::new("app.mds"), app_src)
        .expect("Failed to build module graph");

    let result = jit_run_graph(&graph);
    assert!(result.is_ok(), "JIT run failed: {:?}", result.err());
}
