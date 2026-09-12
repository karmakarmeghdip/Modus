//! LLVM Type mapping and struct memory layouts for Modus.
//!
//! Canonical rules (docs/SPEC.md):
//! - Primitives unboxed: integers, floats, bools mapped directly to native LLVM integer/float types.
//! - Every heap object = `[u64 rc | payload]`: records, arrays, strings, closures, variants.
//! - Heap values are represented as LLVM opaque pointers (`ptr`).
//! - Struct memory layouts prefix all payload fields with an `i64` reference count header.

use crate::ast::PrimitiveType;
use crate::typechecker::Type;
use inkwell::AddressSpace;
use inkwell::context::Context;
use inkwell::types::{BasicMetadataTypeEnum, BasicType, BasicTypeEnum, FunctionType, StructType};
use std::collections::{BTreeMap, HashMap};

/// Translates Modus semantic types to LLVM IR types.
pub struct TypeLowerer<'ctx> {
    pub context: &'ctx Context,
    record_structs: HashMap<String, StructType<'ctx>>,
}

impl<'ctx> TypeLowerer<'ctx> {
    pub fn new(context: &'ctx Context) -> Self {
        Self {
            context,
            record_structs: HashMap::new(),
        }
    }

    /// Maps a Modus type to an LLVM `BasicTypeEnum`.
    ///
    /// Primitives are unboxed (native types).
    /// All heap-allocated types (String, Array, Record, Union, Closure) are `ptr`.
    pub fn llvm_type(&self, ty: &Type) -> BasicTypeEnum<'ctx> {
        match ty {
            Type::Primitive(prim) => match prim {
                PrimitiveType::U8 | PrimitiveType::I8 => {
                    self.context.i8_type().as_basic_type_enum()
                }
                PrimitiveType::U16 | PrimitiveType::I16 => {
                    self.context.i16_type().as_basic_type_enum()
                }
                PrimitiveType::U32 | PrimitiveType::I32 => {
                    self.context.i32_type().as_basic_type_enum()
                }
                PrimitiveType::U64 | PrimitiveType::I64 => {
                    self.context.i64_type().as_basic_type_enum()
                }
                PrimitiveType::F32 => self.context.f32_type().as_basic_type_enum(),
                PrimitiveType::F64 => self.context.f64_type().as_basic_type_enum(),
                PrimitiveType::Bool => self.context.bool_type().as_basic_type_enum(),
                PrimitiveType::String => {
                    // Strings are heap-allocated objects: [u64 rc | len | ptr bytes]
                    self.context
                        .ptr_type(AddressSpace::default())
                        .as_basic_type_enum()
                }
                PrimitiveType::Void => self.context.i8_type().as_basic_type_enum(),
            },
            Type::Unit => self.context.i8_type().as_basic_type_enum(),
            // All heap objects are manipulated via pointers
            Type::Array(_)
            | Type::Record(_)
            | Type::Tuple(_)
            | Type::Named { .. }
            | Type::Function { .. }
            | Type::TraitObject(_)
            | Type::Var(_)
            | Type::GenericParam(_) => self
                .context
                .ptr_type(AddressSpace::default())
                .as_basic_type_enum(),
        }
    }

    /// Maps a Modus return type to an LLVM return type (None for void).
    pub fn llvm_return_type(&self, ty: &Type) -> Option<BasicTypeEnum<'ctx>> {
        if ty.is_void() || *ty == Type::Unit {
            None
        } else {
            Some(self.llvm_type(ty))
        }
    }

    /// Constructs the LLVM struct layout for a Modus record: `{ i64 rc, field_1, field_2, ... }`.
    /// The reference count header `u64 rc` is always field index 0.
    pub fn record_struct_type(&mut self, fields: &BTreeMap<String, Type>) -> StructType<'ctx> {
        let key = format!("{:?}", fields);
        if let Some(st) = self.record_structs.get(&key) {
            return *st;
        }

        let mut field_types: Vec<BasicTypeEnum<'ctx>> = Vec::with_capacity(fields.len() + 1);
        // Field 0: u64 rc
        field_types.push(self.context.i64_type().into());
        // Fields 1..n: payload
        for f_ty in fields.values() {
            field_types.push(self.llvm_type(f_ty));
        }

        let st = self.context.opaque_struct_type("Record");
        st.set_body(&field_types, false);
        self.record_structs.insert(key, st);
        st
    }

    /// Constructs the LLVM struct layout for a Modus Array: `{ i64 rc, i64 len, i64 cap, ptr data }`.
    pub fn array_struct_type(&self) -> StructType<'ctx> {
        self.context.struct_type(
            &[
                self.context.i64_type().into(),                        // rc
                self.context.i64_type().into(),                        // len
                self.context.i64_type().into(),                        // cap
                self.context.ptr_type(AddressSpace::default()).into(), // data
            ],
            false,
        )
    }

    /// Constructs the LLVM struct layout for a Closure object: `{ i64 rc, ptr fn_ptr, ptr env_ptr }`.
    pub fn closure_struct_type(&self) -> StructType<'ctx> {
        self.context.struct_type(
            &[
                self.context.i64_type().into(),                        // rc
                self.context.ptr_type(AddressSpace::default()).into(), // fn_ptr
                self.context.ptr_type(AddressSpace::default()).into(), // env_ptr
            ],
            false,
        )
    }

    /// Constructs the LLVM struct layout for a Discriminated Union Variant:
    /// `{ i64 rc, i32 tag, ...fields... }`.
    pub fn variant_struct_type(&self, field_types: &[Type]) -> StructType<'ctx> {
        let mut llvm_fields: Vec<BasicTypeEnum<'ctx>> = Vec::with_capacity(field_types.len() + 2);
        llvm_fields.push(self.context.i64_type().into()); // rc
        llvm_fields.push(self.context.i32_type().into()); // tag
        for f_ty in field_types {
            llvm_fields.push(self.llvm_type(f_ty));
        }
        self.context.struct_type(&llvm_fields, false)
    }

    /// Computes the LLVM struct element index for a record field.
    /// Since index 0 is always the `rc` header, the field at position `idx` is at `idx + 1`.
    pub fn record_field_index(fields: &BTreeMap<String, Type>, field_name: &str) -> Option<u32> {
        fields
            .keys()
            .position(|k| k == field_name)
            .map(|idx| (idx + 1) as u32)
    }

    /// Constructs an LLVM function type from Modus parameter types and return type.
    pub fn function_type(&self, param_types: &[Type], ret_type: &Type) -> FunctionType<'ctx> {
        let llvm_params: Vec<BasicMetadataTypeEnum<'ctx>> = param_types
            .iter()
            .map(|t| self.llvm_type(t).into())
            .collect();

        if let Some(ret) = self.llvm_return_type(ret_type) {
            ret.fn_type(&llvm_params, false)
        } else {
            self.context.void_type().fn_type(&llvm_params, false)
        }
    }
}
