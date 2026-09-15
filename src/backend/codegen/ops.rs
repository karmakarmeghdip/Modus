//! Primitive evaluation, operator lowering, and type coercion.

use super::CodeGen;
use crate::ast::{BinaryOp, Literal};
use crate::desugar::DesugaredUnaryOp;
use crate::ir::node::Atom;
use inkwell::AddressSpace;
use inkwell::FloatPredicate;
use inkwell::IntPredicate;
use inkwell::types::BasicTypeEnum;
use inkwell::values::{BasicValue, BasicValueEnum, PointerValue};

/// Mangled symbol of `std:string.concat`.
///
/// Calls to this symbol are NOT emitted as calls: `concat` is a codegen-owned
/// intrinsic and `build_string_concat` emits the concatenation inline from
/// allowed building blocks (`modus_alloc`, `memcpy`, `modus_dec_ref`).
/// Inlining (rather than calling) is load-bearing: the prelude's
/// `impl Add for String` calls `concat`, and `concat`'s body lowers to the
/// `Add` impl call, so emitting a real call here would recurse forever.
/// Pinned by `test_string_concat_symbol_matches_intrinsic`.
pub(crate) const STRING_CONCAT_INTRINSIC: &str = "_modus_M_std_string_concat";

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
                    let str_val = self.get_or_create_string_literal(s);
                    Ok(str_val.as_basic_value_enum())
                }
                Literal::Unit => Ok(self.context.i8_type().const_int(0, false).into()),
            },
        }
    }

    /// Returns a pointer to a static immortal Modus string struct:
    /// `{ i64 rc = -1, i64 len, i64 cap, [N+1 x i8] data }`.
    pub(crate) fn get_or_create_string_literal(&self, s: &str) -> PointerValue<'ctx> {
        if let Some(&ptr) = self.string_literals.borrow().get(s) {
            return ptr;
        }

        let i64_type = self.context.i64_type();
        let i8_type = self.context.i8_type();
        let bytes = s.as_bytes();
        let len = bytes.len();
        let array_type = i8_type.array_type((len + 1) as u32);

        let rc_val = i64_type.const_int(-1i64 as u64, true);
        let len_val = i64_type.const_int(len as u64, false);
        let cap_val = i64_type.const_int(len as u64, false);

        let mut char_vals: Vec<inkwell::values::IntValue<'ctx>> = Vec::with_capacity(len + 1);
        for &b in bytes {
            char_vals.push(i8_type.const_int(b as u64, false));
        }
        char_vals.push(i8_type.const_int(0, false));
        let data_val = i8_type.const_array(&char_vals);

        let struct_type = self.context.struct_type(
            &[
                i64_type.into(),
                i64_type.into(),
                i64_type.into(),
                array_type.into(),
            ],
            false,
        );

        let const_struct = struct_type.const_named_struct(&[
            rc_val.into(),
            len_val.into(),
            cap_val.into(),
            data_val.into(),
        ]);

        let global_var =
            self.module
                .add_global(struct_type, Some(AddressSpace::default()), "modus_str_lit");
        global_var.set_initializer(&const_struct);
        global_var.set_constant(true);
        global_var.set_linkage(inkwell::module::Linkage::Internal);

        let ptr = global_var.as_pointer_value();
        self.string_literals.borrow_mut().insert(s.to_string(), ptr);
        ptr
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
            // Note: `String + String` never reaches here: desugar lowers it
            // through the `Add` impl to the inlined `concat` intrinsic above.
            // Pointer `Add` is rejected by the typechecker; fail loudly.
            if op == BinaryOp::Add {
                return Err(
                    "Unsupported pointer + pointer: String `+` must lower via the `Add` impl"
                        .to_string(),
                );
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
        } else if lhs.is_pointer_value() && rhs.is_int_value() {
            let l = self
                .builder
                .build_ptr_to_int(
                    lhs.into_pointer_value(),
                    rhs.into_int_value().get_type(),
                    "ptri",
                )
                .unwrap();
            self.compile_binary_op(op, l.into(), rhs)
        } else if lhs.is_int_value() && rhs.is_pointer_value() {
            let r = self
                .builder
                .build_ptr_to_int(
                    rhs.into_pointer_value(),
                    lhs.into_int_value().get_type(),
                    "ptri",
                )
                .unwrap();
            self.compile_binary_op(op, lhs, r.into())
        } else {
            Err(format!(
                "Mismatched or unsupported binary operand types: lhs={lhs:?}, rhs={rhs:?}, op={op:?}"
            ))
        }
    }

    /// If `callee` is the `concat` intrinsic (`STRING_CONCAT_INTRINSIC`),
    /// emits the concatenation inline and returns `Some(value)`.
    /// Returns `None` for ordinary calls.
    ///
    /// Must be consulted at EVERY call-emission site (`Call` and `TailCall`):
    /// inlining is load-bearing for termination (see the const docs).
    pub(crate) fn try_build_string_concat_call(
        &mut self,
        callee: &Atom,
        args: &[Atom],
    ) -> Option<Result<BasicValueEnum<'ctx>, String>> {
        let Atom::Var(name) = callee else {
            return None;
        };
        if name != STRING_CONCAT_INTRINSIC || args.len() != 2 {
            return None;
        }
        let ptr_ty = self.context.ptr_type(AddressSpace::default());
        let to_ptr = |v: BasicValueEnum<'ctx>| {
            if v.is_pointer_value() {
                v.into_pointer_value()
            } else {
                self.builder
                    .build_int_to_ptr(v.into_int_value(), ptr_ty, "concat_arg")
                    .unwrap()
            }
        };
        let s1 = match self.eval_atom(&args[0]) {
            Ok(v) => to_ptr(v),
            Err(e) => return Some(Err(e)),
        };
        let s2 = match self.eval_atom(&args[1]) {
            Ok(v) => to_ptr(v),
            Err(e) => return Some(Err(e)),
        };
        Some(self.build_string_concat(s1, s2).map(|p| p.into()))
    }

    /// Emits a string concatenation inline: `concat(s1, s2) -> ptr`.
    ///
    /// Same layout and semantics as the deleted `modus_str_concat` runtime
    /// helper, built only from allowed blocks (`modus_alloc`, `memcpy`,
    /// `modus_dec_ref`). String layout: `[rc:i64 | len:i64 | cap:i64 | data]`
    /// with a NUL terminator. Null-tolerant (null reads as empty).
    ///
    /// Ownership (Perceus `Call` model consumes all args): BOTH `s1` and `s2`
    /// are consumed — the FBIP path transfers `s1` into the result and drops
    /// `s2`; the alloc path drops both and returns a fresh buffer. The caller
    /// must not touch either afterwards.
    pub(crate) fn build_string_concat(
        &mut self,
        s1: PointerValue<'ctx>,
        s2: PointerValue<'ctx>,
    ) -> Result<PointerValue<'ctx>, String> {
        let i8_ptr = self.context.ptr_type(AddressSpace::default());
        let i64_type = self.context.i64_type();
        let i8_type = self.context.i8_type();
        let cur_fn = self.current_fn.unwrap();
        let zero = i64_type.const_int(0, false);

        let gep_i64 = |b: &inkwell::builder::Builder<'ctx>,
                       base: PointerValue<'ctx>,
                       idx: u64,
                       name: &str| unsafe {
            b.build_gep(i64_type, base, &[i64_type.const_int(idx, false)], name)
                .unwrap()
        };
        let gep_i8_off = |b: &inkwell::builder::Builder<'ctx>,
                          base: PointerValue<'ctx>,
                          off: u64,
                          name: &str| unsafe {
            b.build_gep(i8_type, base, &[i64_type.const_int(off, false)], name)
                .unwrap()
        };

        let entry_bb = self.builder.get_insert_block().unwrap();
        let s1_load_bb = self.context.append_basic_block(cur_fn, "strcat_s1_load");
        let s1_cont_bb = self.context.append_basic_block(cur_fn, "strcat_s1_cont");
        let s2_load_bb = self.context.append_basic_block(cur_fn, "strcat_s2_load");
        let s2_cont_bb = self.context.append_basic_block(cur_fn, "strcat_s2_cont");
        let check_fbip_bb = self.context.append_basic_block(cur_fn, "strcat_check_fbip");
        let fbip_bb = self.context.append_basic_block(cur_fn, "strcat_fbip");
        let alloc_bb = self.context.append_basic_block(cur_fn, "strcat_alloc");
        let merge_bb = self.context.append_basic_block(cur_fn, "strcat_merge");

        let s1_int = self
            .builder
            .build_ptr_to_int(s1, i64_type, "s1_int")
            .unwrap();
        let s1_is_null = self
            .builder
            .build_int_compare(IntPredicate::EQ, s1_int, zero, "s1_is_null")
            .unwrap();
        let _ = self
            .builder
            .build_conditional_branch(s1_is_null, s1_cont_bb, s1_load_bb);

        self.builder.position_at_end(s1_load_bb);
        let s1_len_val = self
            .builder
            .build_load(
                i64_type,
                gep_i64(&self.builder, s1, 1, "s1_len_p"),
                "s1_len_val",
            )
            .unwrap()
            .into_int_value();
        let _ = self.builder.build_unconditional_branch(s1_cont_bb);

        self.builder.position_at_end(s1_cont_bb);
        let len1_phi = self.builder.build_phi(i64_type, "len1").unwrap();
        len1_phi.add_incoming(&[(&zero, entry_bb), (&s1_len_val, s1_load_bb)]);
        let len1 = len1_phi.as_basic_value().into_int_value();

        let s2_int = self
            .builder
            .build_ptr_to_int(s2, i64_type, "s2_int")
            .unwrap();
        let s2_is_null = self
            .builder
            .build_int_compare(IntPredicate::EQ, s2_int, zero, "s2_is_null")
            .unwrap();
        let _ = self
            .builder
            .build_conditional_branch(s2_is_null, s2_cont_bb, s2_load_bb);

        self.builder.position_at_end(s2_load_bb);
        let s2_len_val = self
            .builder
            .build_load(
                i64_type,
                gep_i64(&self.builder, s2, 1, "s2_len_p"),
                "s2_len_val",
            )
            .unwrap()
            .into_int_value();
        let _ = self.builder.build_unconditional_branch(s2_cont_bb);

        self.builder.position_at_end(s2_cont_bb);
        let len2_phi = self.builder.build_phi(i64_type, "len2").unwrap();
        len2_phi.add_incoming(&[(&zero, s1_cont_bb), (&s2_len_val, s2_load_bb)]);
        let len2 = len2_phi.as_basic_value().into_int_value();

        let total_len = self.builder.build_int_add(len1, len2, "total_len").unwrap();

        // Check FBIP: s1 not null, rc1 == 1 (exactly: never reuse immortal
        // literals), cap1 - len1 >= len2.
        let _ = self
            .builder
            .build_conditional_branch(s1_is_null, alloc_bb, check_fbip_bb);

        self.builder.position_at_end(check_fbip_bb);
        let rc1 = self
            .builder
            .build_load(i64_type, s1, "rc1")
            .unwrap()
            .into_int_value();
        let is_uniq = self
            .builder
            .build_int_compare(
                IntPredicate::EQ,
                rc1,
                i64_type.const_int(1, false),
                "is_uniq",
            )
            .unwrap();
        let cap1 = self
            .builder
            .build_load(i64_type, gep_i64(&self.builder, s1, 2, "cap1_p"), "cap1")
            .unwrap()
            .into_int_value();
        let avail = self.builder.build_int_sub(cap1, len1, "avail").unwrap();
        let can_fit = self
            .builder
            .build_int_compare(IntPredicate::SGE, avail, len2, "can_fit")
            .unwrap();
        let fbip_ok = self.builder.build_and(is_uniq, can_fit, "fbip_ok").unwrap();
        let _ = self
            .builder
            .build_conditional_branch(fbip_ok, fbip_bb, alloc_bb);

        // FBIP append path: s1 is transferred into the result; s2 is dropped.
        self.builder.position_at_end(fbip_bb);
        let data1_fbip = gep_i8_off(&self.builder, s1, 24, "data1_fbip");
        let dest_fbip = unsafe {
            self.builder
                .build_gep(i8_type, data1_fbip, &[len1], "dest_fbip")
                .unwrap()
        };
        let data2_fbip = gep_i8_off(&self.builder, s2, 24, "data2_fbip");
        let _ = self.builder.build_call(
            self.runtime.memcpy_fn,
            &[dest_fbip.into(), data2_fbip.into(), len2.into()],
            "",
        );
        let term_fbip = unsafe {
            self.builder
                .build_gep(i8_type, data1_fbip, &[total_len], "term_fbip")
                .unwrap()
        };
        let _ = self
            .builder
            .build_store(term_fbip, i8_type.const_int(0, false));
        let _ = self
            .builder
            .build_store(gep_i64(&self.builder, s1, 1, "len1_p_fbip"), total_len);
        let _ = self
            .builder
            .build_call(self.runtime.dec_ref_fn, &[s2.into()], "");
        let _ = self.builder.build_unconditional_branch(merge_bb);

        // Alloc path: fresh buffer; both inputs are dropped.
        self.builder.position_at_end(alloc_bb);
        let cap_headroom = self
            .builder
            .build_int_add(total_len, i64_type.const_int(16, false), "cap_headroom")
            .unwrap();
        let alloc_bytes = self
            .builder
            .build_int_add(
                cap_headroom,
                i64_type.const_int(24 + 1, false),
                "alloc_bytes",
            )
            .unwrap();
        let buf = self
            .builder
            .build_call(self.runtime.alloc_fn, &[alloc_bytes.into()], "buf")
            .unwrap()
            .try_as_basic_value()
            .basic()
            .unwrap()
            .into_pointer_value();

        let _ = self
            .builder
            .build_store(gep_i64(&self.builder, buf, 1, "buf_len_p"), total_len);
        let _ = self
            .builder
            .build_store(gep_i64(&self.builder, buf, 2, "buf_cap_p"), cap_headroom);

        let buf_data = gep_i8_off(&self.builder, buf, 24, "buf_data");

        let s1_data = gep_i8_off(&self.builder, s1, 24, "s1_data");
        let safe_s1_data = self
            .builder
            .build_select(s1_is_null, buf_data, s1_data, "safe_s1_data")
            .unwrap()
            .into_pointer_value();
        let _ = self.builder.build_call(
            self.runtime.memcpy_fn,
            &[buf_data.into(), safe_s1_data.into(), len1.into()],
            "",
        );

        let dest2 = unsafe {
            self.builder
                .build_gep(i8_type, buf_data, &[len1], "dest2")
                .unwrap()
        };
        let s2_data = gep_i8_off(&self.builder, s2, 24, "s2_data");
        let safe_s2_data = self
            .builder
            .build_select(s2_is_null, buf_data, s2_data, "safe_s2_data")
            .unwrap()
            .into_pointer_value();
        let _ = self.builder.build_call(
            self.runtime.memcpy_fn,
            &[dest2.into(), safe_s2_data.into(), len2.into()],
            "",
        );

        let term = unsafe {
            self.builder
                .build_gep(i8_type, buf_data, &[total_len], "term")
                .unwrap()
        };
        let _ = self.builder.build_store(term, i8_type.const_int(0, false));

        let _ = self
            .builder
            .build_call(self.runtime.dec_ref_fn, &[s1.into()], "");
        let _ = self
            .builder
            .build_call(self.runtime.dec_ref_fn, &[s2.into()], "");
        let _ = self.builder.build_unconditional_branch(merge_bb);

        self.builder.position_at_end(merge_bb);
        let phi = self.builder.build_phi(i8_ptr, "strcat_res").unwrap();
        phi.add_incoming(&[(&s1, fbip_bb), (&buf, alloc_bb)]);
        Ok(phi.as_basic_value().into_pointer_value())
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
