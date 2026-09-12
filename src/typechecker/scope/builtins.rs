//! Builtin types and prelude registration for Modus.

use crate::ast::{self, PrimitiveType, Span};
use crate::typechecker::scope::Environment;
use crate::typechecker::scope::defs::{
    ConstructorInfo, FunctionSig, TraitDef, TypeDefInfo, VariantInfo,
};
use crate::typechecker::types::Type;
use std::collections::HashMap;

impl Environment {
    pub(crate) fn init_builtins(&mut self) {
        // Register primitive types
        let prims = [
            ("u8", PrimitiveType::U8),
            ("u16", PrimitiveType::U16),
            ("u32", PrimitiveType::U32),
            ("u64", PrimitiveType::U64),
            ("i8", PrimitiveType::I8),
            ("i16", PrimitiveType::I16),
            ("i32", PrimitiveType::I32),
            ("i64", PrimitiveType::I64),
            ("f32", PrimitiveType::F32),
            ("f64", PrimitiveType::F64),
            ("bool", PrimitiveType::Bool),
            ("String", PrimitiveType::String),
            ("void", PrimitiveType::Void),
        ];
        for (name, prim) in prims {
            self.types
                .insert(name.to_string(), TypeDefInfo::Primitive(prim));
        }

        // Register built-in IO type
        self.types.insert(
            "IO".to_string(),
            TypeDefInfo::Builtin {
                name: "IO".to_string(),
                type_params: vec![ast::TypeParam {
                    name: "T".to_string(),
                    bound: None,
                }],
            },
        );

        // Register built-in Pointer(T) type
        self.types.insert(
            "Pointer".to_string(),
            TypeDefInfo::Builtin {
                name: "Pointer".to_string(),
                type_params: vec![ast::TypeParam {
                    name: "T".to_string(),
                    bound: None,
                }],
            },
        );

        // Register built-in CString type alias: Pointer(u8)
        self.types.insert(
            "CString".to_string(),
            TypeDefInfo::Alias {
                name: "CString".to_string(),
                type_params: vec![],
                expanded_type: Type::pointer(Type::u8()),
            },
        );

        // Register IO.pure constructor: (T) => IO(T)
        self.constructors.insert(
            "IO.pure".to_string(),
            ConstructorInfo::Function {
                params: vec![Type::GenericParam("T".to_string())],
                return_type: Type::io(Type::GenericParam("T".to_string())),
                type_params: vec![ast::TypeParam {
                    name: "T".to_string(),
                    bound: None,
                }],
            },
        );

        // Register Option(T) = Some(T) | None
        self.register_option_builtin();

        // Register Result(T, E) = Ok(T) | Err(E)
        self.register_result_builtin();

        // Register ControlFlow(Residual, Output)
        self.types.insert(
            "ControlFlow".to_string(),
            TypeDefInfo::Builtin {
                name: "ControlFlow".to_string(),
                type_params: vec![
                    ast::TypeParam {
                        name: "Residual".to_string(),
                        bound: None,
                    },
                    ast::TypeParam {
                        name: "Output".to_string(),
                        bound: None,
                    },
                ],
            },
        );

        // Register built-in Drawable trait
        let mut drawable_methods = HashMap::new();
        drawable_methods.insert(
            "draw".to_string(),
            FunctionSig {
                name: "draw".to_string(),
                type_params: vec![ast::TypeParam {
                    name: "Self".to_string(),
                    bound: None,
                }],
                params: vec![("self".to_string(), Type::GenericParam("Self".to_string()))],
                return_type: Type::io(Type::void()),
                is_effectful: true,
                span: Span::default(),
                symbol_name: None,
            },
        );
        self.traits.insert(
            "Drawable".to_string(),
            TraitDef {
                name: "Drawable".to_string(),
                type_params: vec![ast::TypeParam {
                    name: "Self".to_string(),
                    bound: None,
                }],
                methods: drawable_methods,
            },
        );
        self.types.insert(
            "Drawable".to_string(),
            TypeDefInfo::Builtin {
                name: "Drawable".to_string(),
                type_params: vec![ast::TypeParam {
                    name: "Self".to_string(),
                    bound: None,
                }],
            },
        );
    }

    pub(crate) fn register_option_builtin(&mut self) {
        let t_param = ast::TypeParam {
            name: "T".to_string(),
            bound: None,
        };
        self.types.insert(
            "Option".to_string(),
            TypeDefInfo::Union {
                name: "Option".to_string(),
                type_params: vec![t_param.clone()],
                variants: vec![
                    VariantInfo {
                        name: "Some".to_string(),
                        fields: vec![Type::GenericParam("T".to_string())],
                    },
                    VariantInfo {
                        name: "None".to_string(),
                        fields: vec![],
                    },
                ],
            },
        );

        let some_ctor = ConstructorInfo::Function {
            params: vec![Type::GenericParam("T".to_string())],
            return_type: Type::option(Type::GenericParam("T".to_string())),
            type_params: vec![t_param.clone()],
        };
        self.constructors
            .insert("Some".to_string(), some_ctor.clone());
        self.constructors
            .insert("Option.Some".to_string(), some_ctor);

        let none_ctor = ConstructorInfo::Value {
            parent_type: Type::option(Type::GenericParam("T".to_string())),
            type_params: vec![t_param],
        };
        self.constructors
            .insert("None".to_string(), none_ctor.clone());
        self.constructors
            .insert("Option.None".to_string(), none_ctor);
    }

    pub(crate) fn register_result_builtin(&mut self) {
        let t_param = ast::TypeParam {
            name: "T".to_string(),
            bound: None,
        };
        let e_param = ast::TypeParam {
            name: "E".to_string(),
            bound: None,
        };
        self.types.insert(
            "Result".to_string(),
            TypeDefInfo::Union {
                name: "Result".to_string(),
                type_params: vec![t_param.clone(), e_param.clone()],
                variants: vec![
                    VariantInfo {
                        name: "Ok".to_string(),
                        fields: vec![Type::GenericParam("T".to_string())],
                    },
                    VariantInfo {
                        name: "Err".to_string(),
                        fields: vec![Type::GenericParam("E".to_string())],
                    },
                ],
            },
        );

        let ok_ctor = ConstructorInfo::Function {
            params: vec![Type::GenericParam("T".to_string())],
            return_type: Type::result(
                Type::GenericParam("T".to_string()),
                Type::GenericParam("E".to_string()),
            ),
            type_params: vec![t_param.clone(), e_param.clone()],
        };
        self.constructors.insert("Ok".to_string(), ok_ctor.clone());
        self.constructors.insert("Result.Ok".to_string(), ok_ctor);

        let err_ctor = ConstructorInfo::Function {
            params: vec![Type::GenericParam("E".to_string())],
            return_type: Type::result(
                Type::GenericParam("T".to_string()),
                Type::GenericParam("E".to_string()),
            ),
            type_params: vec![t_param, e_param],
        };
        self.constructors
            .insert("Err".to_string(), err_ctor.clone());
        self.constructors.insert("Result.Err".to_string(), err_ctor);
    }
}
