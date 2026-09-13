//! Primitive evaluation, operator lowering, and type coercion.

use super::CodeGen;
use crate::ast::{BinaryOp, Literal};
use crate::desugar::DesugaredUnaryOp;
use crate::ir::node::Atom;
use inkwell::AddressSpace;
use inkwell::FloatPredicate;
use inkwell::IntPredicate;
use inkwell::types::BasicTypeEnum;
use inkwell::values::{BasicValue, BasicValueEnum};

impl<'ctx> CodeGen<'ctx> {
    /// Evaluates an `Atom` to its corresponding LLVM `BasicValueEnum`.
    pub(crate) fn eval_atom(&self, atom: &Atom) -> Result<BasicValueEnum<'ctx>, String> {
        match atom {
            Atom::Var(name) => self
                .variables
                .get(name)
                .copied()
                .ok_or_else(|| format!("Undefined variable: {name}")),

            Atom::Literal(lit) => match lit {
                Literal::Int(i) => Ok(self.context.i64_type().const_int(*i as u64, true).into()),
                Literal::UInt(u) => Ok(self.context.i64_type().const_int(*u, false).into()),
                Literal::Float(f) => Ok(self.context.f64_type().const_float(*f).into()),
                Literal::Bool(b) => Ok(self
                    .context
                    .bool_type()
                    .const_int(if *b { 1 } else { 0 }, false)
                    .into()),
                Literal::String(s) => {
                    let str_val = self.builder.build_global_string_ptr(s, "str").unwrap();
                    Ok(str_val.as_basic_value_enum())
                }
                Literal::Unit => Ok(self.context.i8_type().const_int(0, false).into()),
            },
        }
    }

    /// Compiles a binary operation between two operands.
    pub(crate) fn compile_binary_op(
        &self,
        op: BinaryOp,
        lhs: BasicValueEnum<'ctx>,
        rhs: BasicValueEnum<'ctx>,
    ) -> Result<BasicValueEnum<'ctx>, String> {
        if lhs.is_int_value() && rhs.is_int_value() {
            let mut l = lhs.into_int_value();
            let mut r = rhs.into_int_value();
            let l_bits = l.get_type().get_bit_width();
            let r_bits = r.get_type().get_bit_width();
            if l_bits < r_bits {
                r = self
                    .builder
                    .build_int_truncate(r, l.get_type(), "trunc")
                    .unwrap();
            } else if r_bits < l_bits {
                l = self
                    .builder
                    .build_int_truncate(l, r.get_type(), "trunc")
                    .unwrap();
            }

            let res = match op {
                BinaryOp::Add => self.builder.build_int_add(l, r, "add").unwrap().into(),
                BinaryOp::Sub => self.builder.build_int_sub(l, r, "sub").unwrap().into(),
                BinaryOp::Mul => self.builder.build_int_mul(l, r, "mul").unwrap().into(),
                BinaryOp::Div => self
                    .builder
                    .build_int_signed_div(l, r, "div")
                    .unwrap()
                    .into(),
                BinaryOp::Rem => self
                    .builder
                    .build_int_signed_rem(l, r, "rem")
                    .unwrap()
                    .into(),
                BinaryOp::Eq => self
                    .builder
                    .build_int_compare(IntPredicate::EQ, l, r, "eq")
                    .unwrap()
                    .into(),
                BinaryOp::NotEq => self
                    .builder
                    .build_int_compare(IntPredicate::NE, l, r, "ne")
                    .unwrap()
                    .into(),
                BinaryOp::Lt => self
                    .builder
                    .build_int_compare(IntPredicate::SLT, l, r, "lt")
                    .unwrap()
                    .into(),
                BinaryOp::LtEq => self
                    .builder
                    .build_int_compare(IntPredicate::SLE, l, r, "le")
                    .unwrap()
                    .into(),
                BinaryOp::Gt => self
                    .builder
                    .build_int_compare(IntPredicate::SGT, l, r, "gt")
                    .unwrap()
                    .into(),
                BinaryOp::GtEq => self
                    .builder
                    .build_int_compare(IntPredicate::SGE, l, r, "ge")
                    .unwrap()
                    .into(),
                BinaryOp::And => self.builder.build_and(l, r, "and").unwrap().into(),
                BinaryOp::Or => self.builder.build_or(l, r, "or").unwrap().into(),
            };
            Ok(res)
        } else if lhs.is_float_value() && rhs.is_float_value() {
            let mut l = lhs.into_float_value();
            let mut r = rhs.into_float_value();
            if l.get_type() != r.get_type() {
                if l.get_type() == self.context.f32_type() {
                    l = self
                        .builder
                        .build_float_ext(l, self.context.f64_type(), "fpext")
                        .unwrap();
                } else {
                    r = self
                        .builder
                        .build_float_ext(r, self.context.f64_type(), "fpext")
                        .unwrap();
                }
            }

            let res = match op {
                BinaryOp::Add => self.builder.build_float_add(l, r, "fadd").unwrap().into(),
                BinaryOp::Sub => self.builder.build_float_sub(l, r, "fsub").unwrap().into(),
                BinaryOp::Mul => self.builder.build_float_mul(l, r, "fmul").unwrap().into(),
                BinaryOp::Div => self.builder.build_float_div(l, r, "fdiv").unwrap().into(),
                BinaryOp::Rem => self.builder.build_float_rem(l, r, "frem").unwrap().into(),
                BinaryOp::Eq => self
                    .builder
                    .build_float_compare(FloatPredicate::OEQ, l, r, "feq")
                    .unwrap()
                    .into(),
                BinaryOp::NotEq => self
                    .builder
                    .build_float_compare(FloatPredicate::ONE, l, r, "fne")
                    .unwrap()
                    .into(),
                BinaryOp::Lt => self
                    .builder
                    .build_float_compare(FloatPredicate::OLT, l, r, "flt")
                    .unwrap()
                    .into(),
                BinaryOp::LtEq => self
                    .builder
                    .build_float_compare(FloatPredicate::OLE, l, r, "fle")
                    .unwrap()
                    .into(),
                BinaryOp::Gt => self
                    .builder
                    .build_float_compare(FloatPredicate::OGT, l, r, "fgt")
                    .unwrap()
                    .into(),
                BinaryOp::GtEq => self
                    .builder
                    .build_float_compare(FloatPredicate::OGE, l, r, "fge")
                    .unwrap()
                    .into(),
                _ => return Err(format!("Unsupported float binary op: {op:?}")),
            };
            Ok(res)
        } else if lhs.is_float_value() && rhs.is_int_value() {
            let l = lhs.into_float_value();
            let r = self
                .builder
                .build_signed_int_to_float(rhs.into_int_value(), l.get_type(), "sitofp")
                .unwrap();
            self.compile_binary_op(op, l.into(), r.into())
        } else if lhs.is_int_value() && rhs.is_float_value() {
            let r = rhs.into_float_value();
            let l = self
                .builder
                .build_signed_int_to_float(lhs.into_int_value(), r.get_type(), "sitofp")
                .unwrap();
            self.compile_binary_op(op, l.into(), r.into())
        } else if lhs.is_pointer_value() && rhs.is_pointer_value() {
            if op == BinaryOp::Add {
                let call = self
                    .builder
                    .build_call(
                        self.runtime.str_concat_fn,
                        &[lhs.into(), rhs.into()],
                        "str_concat",
                    )
                    .map_err(|e| e.to_string())?;
                return Ok(call.try_as_basic_value().basic().unwrap_or_else(|| {
                    self.context
                        .ptr_type(AddressSpace::default())
                        .const_null()
                        .into()
                }));
            }
            let l = self
                .builder
                .build_ptr_to_int(lhs.into_pointer_value(), self.context.i64_type(), "ptri")
                .unwrap();
            let r = self
                .builder
                .build_ptr_to_int(rhs.into_pointer_value(), self.context.i64_type(), "ptri")
                .unwrap();
            self.compile_binary_op(op, l.into(), r.into())
        } else {
            Err("Mismatched or unsupported binary operand types".to_string())
        }
    }

    /// Compiles a unary operation.
    pub(crate) fn compile_unary_op(
        &self,
        op: DesugaredUnaryOp,
        val: BasicValueEnum<'ctx>,
    ) -> Result<BasicValueEnum<'ctx>, String> {
        match op {
            DesugaredUnaryOp::Not => {
                let int_val = val.into_int_value();
                Ok(self.builder.build_not(int_val, "not").unwrap().into())
            }
            DesugaredUnaryOp::Neg => {
                if val.is_int_value() {
                    Ok(self
                        .builder
                        .build_int_neg(val.into_int_value(), "neg")
                        .unwrap()
                        .into())
                } else if val.is_float_value() {
                    Ok(self
                        .builder
                        .build_float_neg(val.into_float_value(), "fneg")
                        .unwrap()
                        .into())
                } else {
                    Err("Cannot negate non-numeric value".to_string())
                }
            }
            DesugaredUnaryOp::Perform => {
                // `perform` unwraps IO in the type system; in LLVM IR the value is already computed
                Ok(val)
            }
        }
    }

    /// Coerces a value to the target LLVM type if required.
    pub(crate) fn coerce_to_type(
        &self,
        val: BasicValueEnum<'ctx>,
        target: BasicTypeEnum<'ctx>,
    ) -> Result<BasicValueEnum<'ctx>, String> {
        if val.get_type() == target {
            return Ok(val);
        }
        if val.is_int_value() && target.is_int_type() {
            let val_int = val.into_int_value();
            let exp_int = target.into_int_type();
            if val_int.get_type().get_bit_width() > exp_int.get_bit_width() {
                Ok(self
                    .builder
                    .build_int_truncate(val_int, exp_int, "trunc")
                    .unwrap()
                    .into())
            } else {
                Ok(self
                    .builder
                    .build_int_s_extend_or_bit_cast(val_int, exp_int, "sext")
                    .unwrap()
                    .into())
            }
        } else if val.is_int_value() && target.is_float_type() {
            Ok(self
                .builder
                .build_signed_int_to_float(val.into_int_value(), target.into_float_type(), "itof")
                .unwrap()
                .into())
        } else if val.is_float_value() && target.is_float_type() {
            Ok(self
                .builder
                .build_float_cast(val.into_float_value(), target.into_float_type(), "fcast")
                .unwrap()
                .into())
        } else if val.is_int_value() && target.is_pointer_type() {
            Ok(self
                .builder
                .build_int_to_ptr(val.into_int_value(), target.into_pointer_type(), "itoptr")
                .unwrap()
                .into())
        } else if val.is_pointer_value() && target.is_int_type() {
            Ok(self
                .builder
                .build_ptr_to_int(val.into_pointer_value(), target.into_int_type(), "ptrtoi")
                .unwrap()
                .into())
        } else {
            Ok(val)
        }
    }

    /// Emits a type-conforming return instruction matching the current function's return signature.
    pub(crate) fn build_typed_return(
        &self,
        val: Option<BasicValueEnum<'ctx>>,
    ) -> Result<(), String> {
        let cur_fn = self.current_fn.unwrap();
        let fn_ty = cur_fn.get_type();
        match fn_ty.get_return_type() {
            None => {
                let _ = self.builder.build_return(None);
                Ok(())
            }
            Some(expected_ty) => {
                let actual_val = match val {
                    Some(v) => self.coerce_to_type(v, expected_ty)?,
                    None => match expected_ty {
                        BasicTypeEnum::IntType(t) => t.const_zero().into(),
                        BasicTypeEnum::FloatType(t) => t.const_zero().into(),
                        BasicTypeEnum::PointerType(t) => t.const_null().into(),
                        BasicTypeEnum::StructType(t) => t.const_zero().into(),
                        BasicTypeEnum::ArrayType(t) => t.const_zero().into(),
                        BasicTypeEnum::VectorType(t) => t.const_zero().into(),
                        BasicTypeEnum::ScalableVectorType(t) => t.const_zero().into(),
                    },
                };
                let _ = self.builder.build_return(Some(&actual_val));
                Ok(())
            }
        }
    }
}
