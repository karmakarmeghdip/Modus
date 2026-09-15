//! Runtime support, memory management primitives, and IO stubs for LLVM backend.
//!
//! Implements:
//! - `malloc` and `free` declarations
//! - `modus_alloc`: allocates heap object and initializes `rc = 1`
//! - `modus_inc_ref`: non-atomic reference count increment
//! - `modus_dec_ref`: inline fast-path (`sub` + `icmp eq 0` -> `free`)
//! - `modus_is_unique`: FBIP uniqueness check (`rc == 1`)
//! - Perceus reference counting (`modus_alloc`, `modus_inc_ref`, `modus_dec_ref`, `modus_is_unique`)
//! - String primitives (`concat`, `eq`, `substring`, `from_c_str`, `from_char_code`)

use inkwell::AddressSpace;
use inkwell::IntPredicate;
use inkwell::context::Context;
use inkwell::module::Module;
use inkwell::values::FunctionValue;

/// Manages runtime declarations and helper functions in an LLVM module.
pub struct Runtime<'ctx> {
    pub context: &'ctx Context,
    pub malloc_fn: FunctionValue<'ctx>,
    pub free_fn: FunctionValue<'ctx>,
    pub strlen_fn: FunctionValue<'ctx>,
    pub memcpy_fn: FunctionValue<'ctx>,
    pub snprintf_fn: FunctionValue<'ctx>,
    pub alloc_fn: FunctionValue<'ctx>,
    pub inc_ref_fn: FunctionValue<'ctx>,
    pub dec_ref_fn: FunctionValue<'ctx>,
    pub is_unique_fn: FunctionValue<'ctx>,
    pub str_substring_fn: FunctionValue<'ctx>,
    pub string_from_c_str_fn: FunctionValue<'ctx>,
    pub str_from_char_code_fn: FunctionValue<'ctx>,
    pub array_new_fn: FunctionValue<'ctx>,
    pub array_push_fn: FunctionValue<'ctx>,
    pub array_build_fn: FunctionValue<'ctx>,
    pub array_set_fn: FunctionValue<'ctx>,
    pub array_pop_fn: FunctionValue<'ctx>,
}

impl<'ctx> Runtime<'ctx> {
    pub fn new(context: &'ctx Context, module: &Module<'ctx>) -> Self {
        let i8_ptr = context.ptr_type(AddressSpace::default());
        let i64_type = context.i64_type();
        let i32_type = context.i32_type();
        let void_type = context.void_type();

        // 1. extern void* malloc(size_t size);
        let malloc_fn = module.get_function("malloc").unwrap_or_else(|| {
            let fn_type = i8_ptr.fn_type(&[i64_type.into()], false);
            module.add_function("malloc", fn_type, None)
        });

        // 2. extern void free(void* ptr);
        let free_fn = module.get_function("free").unwrap_or_else(|| {
            let fn_type = void_type.fn_type(&[i8_ptr.into()], false);
            module.add_function("free", fn_type, None)
        });

        // 3. extern size_t strlen(const char* str);
        let strlen_fn = module.get_function("strlen").unwrap_or_else(|| {
            let fn_type = i64_type.fn_type(&[i8_ptr.into()], false);
            module.add_function("strlen", fn_type, None)
        });

        // 6. extern void* memcpy(void* dest, const void* src, size_t n);
        let memcpy_fn = module.get_function("memcpy").unwrap_or_else(|| {
            let fn_type = i8_ptr.fn_type(&[i8_ptr.into(), i8_ptr.into(), i64_type.into()], false);
            module.add_function("memcpy", fn_type, None)
        });

        // 7. extern int snprintf(char* str, size_t size, const char* format, ...);
        let snprintf_fn = module.get_function("snprintf").unwrap_or_else(|| {
            let fn_type = i32_type.fn_type(&[i8_ptr.into(), i64_type.into(), i8_ptr.into()], true);
            module.add_function("snprintf", fn_type, None)
        });

        // 8. Build helper: modus_alloc(size: i64) -> ptr
        let alloc_fn = module
            .get_function("modus_alloc")
            .unwrap_or_else(|| Self::build_alloc_fn(context, module, malloc_fn));

        // 9. Build helper: modus_inc_ref(ptr: ptr) -> void
        let inc_ref_fn = module
            .get_function("modus_inc_ref")
            .unwrap_or_else(|| Self::build_inc_ref_fn(context, module));

        // 10. Build helper: modus_dec_ref(ptr: ptr) -> void
        let dec_ref_fn = module
            .get_function("modus_dec_ref")
            .unwrap_or_else(|| Self::build_dec_ref_fn(context, module, free_fn));

        // 11. Build helper: modus_is_unique(ptr: ptr) -> bool
        let is_unique_fn = module
            .get_function("modus_is_unique")
            .unwrap_or_else(|| Self::build_is_unique_fn(context, module));

        // 15. Build helper: modus_str_substring(s: ptr, start: i64, end: i64) -> ptr
        let str_substring_fn = module
            .get_function("modus_str_substring")
            .unwrap_or_else(|| Self::build_str_substring_fn(context, module, alloc_fn, memcpy_fn));

        // 16. Build helper: modus_string_from_c_str(cs: ptr) -> ptr
        let string_from_c_str_fn = module
            .get_function("modus_string_from_c_str")
            .unwrap_or_else(|| {
                Self::build_string_from_c_str_fn(context, module, alloc_fn, strlen_fn, memcpy_fn)
            });

        // 17. Build helper: modus_str_from_char_code(code: i32) -> ptr
        let str_from_char_code_fn = module
            .get_function("modus_str_from_char_code")
            .unwrap_or_else(|| Self::build_str_from_char_code_fn(context, module, alloc_fn));

        // 18. Build array helpers:
        let array_new_fn = module
            .get_function("modus_array_new")
            .unwrap_or_else(|| Self::build_array_new_fn(context, module, malloc_fn));

        let array_push_fn = module.get_function("modus_array_push").unwrap_or_else(|| {
            Self::build_array_push_fn(
                context, module, malloc_fn, free_fn, memcpy_fn, inc_ref_fn, dec_ref_fn,
            )
        });

        let array_build_fn = module.get_function("modus_array_build").unwrap_or_else(|| {
            Self::build_array_build_fn(
                context, module, malloc_fn, memcpy_fn, inc_ref_fn, dec_ref_fn,
            )
        });

        let array_set_fn = module.get_function("modus_array_set").unwrap_or_else(|| {
            Self::build_array_set_fn(
                context, module, malloc_fn, memcpy_fn, inc_ref_fn, dec_ref_fn,
            )
        });

        let array_pop_fn = module.get_function("modus_array_pop").unwrap_or_else(|| {
            Self::build_array_pop_fn(
                context, module, malloc_fn, memcpy_fn, inc_ref_fn, dec_ref_fn,
            )
        });

        Self {
            context,
            malloc_fn,
            free_fn,
            strlen_fn,
            memcpy_fn,
            snprintf_fn,
            alloc_fn,
            inc_ref_fn,
            dec_ref_fn,
            is_unique_fn,
            str_substring_fn,
            string_from_c_str_fn,
            str_from_char_code_fn,
            array_new_fn,
            array_push_fn,
            array_build_fn,
            array_set_fn,
            array_pop_fn,
        }
    }

    /// Emits `modus_alloc(size: i64) -> ptr`:
    /// Allocates buffer with `malloc(size)`, sets `rc = 1` at offset 0, and returns pointer.
    fn build_alloc_fn(
        context: &'ctx Context,
        module: &Module<'ctx>,
        malloc_fn: FunctionValue<'ctx>,
    ) -> FunctionValue<'ctx> {
        let i8_ptr = context.ptr_type(AddressSpace::default());
        let i64_type = context.i64_type();
        let fn_type = i8_ptr.fn_type(&[i64_type.into()], false);
        let func = module.add_function("modus_alloc", fn_type, None);

        let builder = context.create_builder();
        let entry_bb = context.append_basic_block(func, "entry");
        builder.position_at_end(entry_bb);

        let size_arg = func.get_first_param().unwrap().into_int_value();
        let mem_call = builder
            .build_call(malloc_fn, &[size_arg.into()], "raw_mem")
            .unwrap();
        let mem_ptr = mem_call
            .try_as_basic_value()
            .basic()
            .unwrap()
            .into_pointer_value();

        // Store rc = 1 at offset 0 (i64)
        let rc_one = i64_type.const_int(1, false);
        let _ = builder.build_store(mem_ptr, rc_one);

        let _ = builder.build_return(Some(&mem_ptr));
        func
    }

    /// Emits `modus_inc_ref(ptr: ptr) -> void`:
    /// Non-atomic: loads i64 rc, adds 1, stores back.
    /// If ptr is null or rc <= 0 (immortal, e.g. static string literals), skips increment.
    fn build_inc_ref_fn(context: &'ctx Context, module: &Module<'ctx>) -> FunctionValue<'ctx> {
        let i8_ptr = context.ptr_type(AddressSpace::default());
        let i64_type = context.i64_type();
        let void_type = context.void_type();
        let fn_type = void_type.fn_type(&[i8_ptr.into()], false);
        let func = module.add_function("modus_inc_ref", fn_type, None);

        let builder = context.create_builder();
        let entry_bb = context.append_basic_block(func, "entry");
        let not_null_bb = context.append_basic_block(func, "not_null");
        let inc_bb = context.append_basic_block(func, "inc");
        let ret_bb = context.append_basic_block(func, "ret");

        builder.position_at_end(entry_bb);
        let ptr_arg = func.get_first_param().unwrap().into_pointer_value();
        let ptr_int = builder
            .build_ptr_to_int(ptr_arg, i64_type, "ptr_int")
            .unwrap();
        let is_null = builder
            .build_int_compare(
                IntPredicate::EQ,
                ptr_int,
                i64_type.const_int(0, false),
                "is_null",
            )
            .unwrap();
        let _ = builder.build_conditional_branch(is_null, ret_bb, not_null_bb);

        builder.position_at_end(not_null_bb);
        let rc = builder
            .build_load(i64_type, ptr_arg, "rc")
            .unwrap()
            .into_int_value();
        // Immortal check: if rc <= 0 (e.g. -1 for static literals), do not mutate rodata
        let is_immortal = builder
            .build_int_compare(
                IntPredicate::SLE,
                rc,
                i64_type.const_int(0, false),
                "is_immortal",
            )
            .unwrap();
        let _ = builder.build_conditional_branch(is_immortal, ret_bb, inc_bb);

        builder.position_at_end(inc_bb);
        let rc_plus_1 = builder
            .build_int_add(rc, i64_type.const_int(1, false), "rc_inc")
            .unwrap();
        let _ = builder.build_store(ptr_arg, rc_plus_1);
        let _ = builder.build_unconditional_branch(ret_bb);

        builder.position_at_end(ret_bb);
        let _ = builder.build_return(None);
        func
    }

    /// Emits `modus_dec_ref(ptr: ptr) -> void`:
    /// Inline fast path: `sub` + `icmp eq 0` -> `free(ptr)`.
    /// If ptr is null or rc <= 0 (immortal, e.g. static string literals), skips decrement and free.
    fn build_dec_ref_fn(
        context: &'ctx Context,
        module: &Module<'ctx>,
        free_fn: FunctionValue<'ctx>,
    ) -> FunctionValue<'ctx> {
        let i8_ptr = context.ptr_type(AddressSpace::default());
        let i64_type = context.i64_type();
        let void_type = context.void_type();
        let fn_type = void_type.fn_type(&[i8_ptr.into()], false);
        let func = module.add_function("modus_dec_ref", fn_type, None);

        let builder = context.create_builder();
        let entry_bb = context.append_basic_block(func, "entry");
        let not_null_bb = context.append_basic_block(func, "not_null");
        let dec_bb = context.append_basic_block(func, "dec");
        let free_bb = context.append_basic_block(func, "free_block");
        let ret_bb = context.append_basic_block(func, "ret");

        builder.position_at_end(entry_bb);
        let ptr_arg = func.get_first_param().unwrap().into_pointer_value();

        let is_null = builder
            .build_int_compare(
                IntPredicate::EQ,
                builder
                    .build_ptr_to_int(ptr_arg, i64_type, "ptr_val")
                    .unwrap(),
                i64_type.const_int(0, false),
                "is_null",
            )
            .unwrap();
        let _ = builder.build_conditional_branch(is_null, ret_bb, not_null_bb);

        builder.position_at_end(not_null_bb);
        let rc = builder
            .build_load(i64_type, ptr_arg, "rc")
            .unwrap()
            .into_int_value();
        // Immortal check: if rc <= 0 (e.g. -1 for static literals), do not decrement or free
        let is_immortal = builder
            .build_int_compare(
                IntPredicate::SLE,
                rc,
                i64_type.const_int(0, false),
                "is_immortal",
            )
            .unwrap();
        let _ = builder.build_conditional_branch(is_immortal, ret_bb, dec_bb);

        builder.position_at_end(dec_bb);
        let rc_minus_1 = builder
            .build_int_sub(rc, i64_type.const_int(1, false), "rc_dec")
            .unwrap();
        let _ = builder.build_store(ptr_arg, rc_minus_1);

        let is_zero = builder
            .build_int_compare(
                IntPredicate::EQ,
                rc_minus_1,
                i64_type.const_int(0, false),
                "is_zero",
            )
            .unwrap();
        let _ = builder.build_conditional_branch(is_zero, free_bb, ret_bb);

        // Free block: calls free(ptr)
        builder.position_at_end(free_bb);
        let _ = builder.build_call(free_fn, &[ptr_arg.into()], "");
        let _ = builder.build_unconditional_branch(ret_bb);

        // Ret block: return
        builder.position_at_end(ret_bb);
        let _ = builder.build_return(None);

        func
    }

    /// Emits `modus_is_unique(ptr: ptr) -> bool`:
    /// Returns `true` iff `rc == 1`.
    fn build_is_unique_fn(context: &'ctx Context, module: &Module<'ctx>) -> FunctionValue<'ctx> {
        let i8_ptr = context.ptr_type(AddressSpace::default());
        let i64_type = context.i64_type();
        let bool_type = context.bool_type();
        let fn_type = bool_type.fn_type(&[i8_ptr.into()], false);
        let func = module.add_function("modus_is_unique", fn_type, None);

        let builder = context.create_builder();
        let entry_bb = context.append_basic_block(func, "entry");
        builder.position_at_end(entry_bb);

        let ptr_arg = func.get_first_param().unwrap().into_pointer_value();
        let rc = builder
            .build_load(i64_type, ptr_arg, "rc")
            .unwrap()
            .into_int_value();
        let is_one = builder
            .build_int_compare(
                IntPredicate::EQ,
                rc,
                i64_type.const_int(1, false),
                "is_unique",
            )
            .unwrap();

        let _ = builder.build_return(Some(&is_one));
        func
    }

    /// Emits `modus_str_substring(s: ptr, start: i64, end: i64) -> ptr`:
    fn build_str_substring_fn(
        context: &'ctx Context,
        module: &Module<'ctx>,
        alloc_fn: FunctionValue<'ctx>,
        memcpy_fn: FunctionValue<'ctx>,
    ) -> FunctionValue<'ctx> {
        let i8_ptr = context.ptr_type(AddressSpace::default());
        let i64_type = context.i64_type();
        let i8_type = context.i8_type();
        let fn_type = i8_ptr.fn_type(&[i8_ptr.into(), i64_type.into(), i64_type.into()], false);
        let func = module.add_function("modus_str_substring", fn_type, None);

        let builder = context.create_builder();
        let entry_bb = context.append_basic_block(func, "entry");
        let bounds_bb = context.append_basic_block(func, "bounds");
        let alloc_bb = context.append_basic_block(func, "alloc");
        let ret_empty_bb = context.append_basic_block(func, "ret_empty");

        builder.position_at_end(entry_bb);
        let s = func.get_nth_param(0).unwrap().into_pointer_value();
        let start = func.get_nth_param(1).unwrap().into_int_value();
        let end = func.get_nth_param(2).unwrap().into_int_value();

        let s_int = builder.build_ptr_to_int(s, i64_type, "s_int").unwrap();
        let is_null = builder
            .build_int_compare(
                IntPredicate::EQ,
                s_int,
                i64_type.const_int(0, false),
                "is_null",
            )
            .unwrap();
        let _ = builder.build_conditional_branch(is_null, ret_empty_bb, bounds_bb);

        builder.position_at_end(bounds_bb);
        let len_p = unsafe {
            builder
                .build_gep(i64_type, s, &[i64_type.const_int(1, false)], "len_p")
                .unwrap()
        };
        let s_len = builder
            .build_load(i64_type, len_p, "s_len")
            .unwrap()
            .into_int_value();

        let zero = i64_type.const_int(0, false);
        let st_lt_0 = builder
            .build_int_compare(IntPredicate::SLT, start, zero, "st_lt_0")
            .unwrap();
        let st0 = builder
            .build_select(st_lt_0, zero, start, "st0")
            .unwrap()
            .into_int_value();
        let st_gt_len = builder
            .build_int_compare(IntPredicate::SGT, st0, s_len, "st_gt_len")
            .unwrap();
        let st = builder
            .build_select(st_gt_len, s_len, st0, "st")
            .unwrap()
            .into_int_value();

        let en_lt_0 = builder
            .build_int_compare(IntPredicate::SLT, end, zero, "en_lt_0")
            .unwrap();
        let en0 = builder
            .build_select(en_lt_0, zero, end, "en0")
            .unwrap()
            .into_int_value();
        let en_gt_len = builder
            .build_int_compare(IntPredicate::SGT, en0, s_len, "en_gt_len")
            .unwrap();
        let en = builder
            .build_select(en_gt_len, s_len, en0, "en")
            .unwrap()
            .into_int_value();

        let st_gt_en = builder
            .build_int_compare(IntPredicate::SGT, st, en, "st_gt_en")
            .unwrap();
        let actual_start = builder
            .build_select(st_gt_en, en, st, "actual_start")
            .unwrap()
            .into_int_value();
        let actual_end = builder
            .build_select(st_gt_en, st, en, "actual_end")
            .unwrap()
            .into_int_value();

        let sub_len = builder
            .build_int_sub(actual_end, actual_start, "sub_len")
            .unwrap();
        let is_empty = builder
            .build_int_compare(IntPredicate::SLE, sub_len, zero, "is_empty")
            .unwrap();
        let _ = builder.build_conditional_branch(is_empty, ret_empty_bb, alloc_bb);

        builder.position_at_end(alloc_bb);
        let alloc_bytes = builder
            .build_int_add(sub_len, i64_type.const_int(24 + 1, false), "alloc_bytes")
            .unwrap();
        let buf_call = builder
            .build_call(alloc_fn, &[alloc_bytes.into()], "sub_buf")
            .unwrap();
        let buf = buf_call
            .try_as_basic_value()
            .basic()
            .unwrap()
            .into_pointer_value();

        let buf_len_p = unsafe {
            builder
                .build_gep(i64_type, buf, &[i64_type.const_int(1, false)], "buf_len_p")
                .unwrap()
        };
        let _ = builder.build_store(buf_len_p, sub_len);
        let buf_cap_p = unsafe {
            builder
                .build_gep(i64_type, buf, &[i64_type.const_int(2, false)], "buf_cap_p")
                .unwrap()
        };
        let _ = builder.build_store(buf_cap_p, sub_len);

        let src_off = builder
            .build_int_add(i64_type.const_int(24, false), actual_start, "src_off")
            .unwrap();
        let src_data = unsafe {
            builder
                .build_gep(i8_type, s, &[src_off], "src_data")
                .unwrap()
        };
        let dest_data = unsafe {
            builder
                .build_gep(i8_type, buf, &[i64_type.const_int(24, false)], "dest_data")
                .unwrap()
        };
        let _ = builder.build_call(
            memcpy_fn,
            &[dest_data.into(), src_data.into(), sub_len.into()],
            "",
        );

        let term_p = unsafe {
            builder
                .build_gep(i8_type, dest_data, &[sub_len], "term_p")
                .unwrap()
        };
        let _ = builder.build_store(term_p, i8_type.const_int(0, false));
        let _ = builder.build_return(Some(&buf));

        builder.position_at_end(ret_empty_bb);
        let empty_call = builder
            .build_call(
                alloc_fn,
                &[i64_type.const_int(24 + 1, false).into()],
                "empty_buf",
            )
            .unwrap();
        let empty_buf = empty_call
            .try_as_basic_value()
            .basic()
            .unwrap()
            .into_pointer_value();
        let e_len_p = unsafe {
            builder
                .build_gep(
                    i64_type,
                    empty_buf,
                    &[i64_type.const_int(1, false)],
                    "e_len_p",
                )
                .unwrap()
        };
        let _ = builder.build_store(e_len_p, zero);
        let e_cap_p = unsafe {
            builder
                .build_gep(
                    i64_type,
                    empty_buf,
                    &[i64_type.const_int(2, false)],
                    "e_cap_p",
                )
                .unwrap()
        };
        let _ = builder.build_store(e_cap_p, zero);
        let e_data = unsafe {
            builder
                .build_gep(
                    i8_type,
                    empty_buf,
                    &[i64_type.const_int(24, false)],
                    "e_data",
                )
                .unwrap()
        };
        let _ = builder.build_store(e_data, i8_type.const_int(0, false));
        let _ = builder.build_return(Some(&empty_buf));

        func
    }

    /// Emits `modus_string_from_c_str(cs: ptr) -> ptr`:
    fn build_string_from_c_str_fn(
        context: &'ctx Context,
        module: &Module<'ctx>,
        alloc_fn: FunctionValue<'ctx>,
        strlen_fn: FunctionValue<'ctx>,
        memcpy_fn: FunctionValue<'ctx>,
    ) -> FunctionValue<'ctx> {
        let i8_ptr = context.ptr_type(AddressSpace::default());
        let i64_type = context.i64_type();
        let i8_type = context.i8_type();
        let fn_type = i8_ptr.fn_type(&[i8_ptr.into()], false);
        let func = module.add_function("modus_string_from_c_str", fn_type, None);

        let builder = context.create_builder();
        let entry_bb = context.append_basic_block(func, "entry");
        let alloc_bb = context.append_basic_block(func, "alloc");
        let ret_empty_bb = context.append_basic_block(func, "ret_empty");

        builder.position_at_end(entry_bb);
        let cs = func.get_nth_param(0).unwrap().into_pointer_value();
        let cs_int = builder.build_ptr_to_int(cs, i64_type, "cs_int").unwrap();
        let is_null = builder
            .build_int_compare(
                IntPredicate::EQ,
                cs_int,
                i64_type.const_int(0, false),
                "is_null",
            )
            .unwrap();
        let _ = builder.build_conditional_branch(is_null, ret_empty_bb, alloc_bb);

        builder.position_at_end(alloc_bb);
        let len_call = builder
            .build_call(strlen_fn, &[cs.into()], "c_len")
            .unwrap();
        let len = len_call
            .try_as_basic_value()
            .basic()
            .unwrap()
            .into_int_value();
        let alloc_bytes = builder
            .build_int_add(len, i64_type.const_int(24 + 1, false), "alloc_bytes")
            .unwrap();
        let buf_call = builder
            .build_call(alloc_fn, &[alloc_bytes.into()], "buf")
            .unwrap();
        let buf = buf_call
            .try_as_basic_value()
            .basic()
            .unwrap()
            .into_pointer_value();

        let len_p = unsafe {
            builder
                .build_gep(i64_type, buf, &[i64_type.const_int(1, false)], "len_p")
                .unwrap()
        };
        let _ = builder.build_store(len_p, len);
        let cap_p = unsafe {
            builder
                .build_gep(i64_type, buf, &[i64_type.const_int(2, false)], "cap_p")
                .unwrap()
        };
        let _ = builder.build_store(cap_p, len);

        let dest = unsafe {
            builder
                .build_gep(i8_type, buf, &[i64_type.const_int(24, false)], "dest")
                .unwrap()
        };
        let len_plus_1 = builder
            .build_int_add(len, i64_type.const_int(1, false), "len_plus_1")
            .unwrap();
        let _ = builder.build_call(memcpy_fn, &[dest.into(), cs.into(), len_plus_1.into()], "");
        let _ = builder.build_return(Some(&buf));

        builder.position_at_end(ret_empty_bb);
        let zero = i64_type.const_int(0, false);
        let empty_call = builder
            .build_call(
                alloc_fn,
                &[i64_type.const_int(24 + 1, false).into()],
                "empty_buf",
            )
            .unwrap();
        let empty_buf = empty_call
            .try_as_basic_value()
            .basic()
            .unwrap()
            .into_pointer_value();
        let e_len_p = unsafe {
            builder
                .build_gep(
                    i64_type,
                    empty_buf,
                    &[i64_type.const_int(1, false)],
                    "e_len_p",
                )
                .unwrap()
        };
        let _ = builder.build_store(e_len_p, zero);
        let e_cap_p = unsafe {
            builder
                .build_gep(
                    i64_type,
                    empty_buf,
                    &[i64_type.const_int(2, false)],
                    "e_cap_p",
                )
                .unwrap()
        };
        let _ = builder.build_store(e_cap_p, zero);
        let e_data = unsafe {
            builder
                .build_gep(
                    i8_type,
                    empty_buf,
                    &[i64_type.const_int(24, false)],
                    "e_data",
                )
                .unwrap()
        };
        let _ = builder.build_store(e_data, i8_type.const_int(0, false));
        let _ = builder.build_return(Some(&empty_buf));

        func
    }

    /// Emits `modus_str_from_char_code(code: i32) -> ptr`:
    fn build_str_from_char_code_fn(
        context: &'ctx Context,
        module: &Module<'ctx>,
        alloc_fn: FunctionValue<'ctx>,
    ) -> FunctionValue<'ctx> {
        let i8_ptr = context.ptr_type(AddressSpace::default());
        let i64_type = context.i64_type();
        let i32_type = context.i32_type();
        let i8_type = context.i8_type();
        let fn_type = i8_ptr.fn_type(&[i32_type.into()], false);
        let func = module.add_function("modus_str_from_char_code", fn_type, None);

        let builder = context.create_builder();
        let entry_bb = context.append_basic_block(func, "entry");
        builder.position_at_end(entry_bb);

        let code = func.get_nth_param(0).unwrap().into_int_value();
        let code_u8 = builder
            .build_int_truncate(code, i8_type, "code_u8")
            .unwrap();

        let alloc_bytes = i64_type.const_int(24 + 1 + 1, false);
        let buf_call = builder
            .build_call(alloc_fn, &[alloc_bytes.into()], "char_buf")
            .unwrap();
        let buf = buf_call
            .try_as_basic_value()
            .basic()
            .unwrap()
            .into_pointer_value();

        let one = i64_type.const_int(1, false);
        let len_p = unsafe { builder.build_gep(i64_type, buf, &[one], "len_p").unwrap() };
        let _ = builder.build_store(len_p, one);
        let cap_p = unsafe {
            builder
                .build_gep(i64_type, buf, &[i64_type.const_int(2, false)], "cap_p")
                .unwrap()
        };
        let _ = builder.build_store(cap_p, one);

        let dest = unsafe {
            builder
                .build_gep(i8_type, buf, &[i64_type.const_int(24, false)], "dest")
                .unwrap()
        };
        let _ = builder.build_store(dest, code_u8);
        let term = unsafe { builder.build_gep(i8_type, dest, &[one], "term").unwrap() };
        let _ = builder.build_store(term, i8_type.const_int(0, false));

        let _ = builder.build_return(Some(&buf));
        func
    }

    /// Emits `modus_array_new(cap: i64) -> ptr`:
    /// Allocates buffer of size (4 + max(cap, 4)) * 8 bytes, sets rc = 1, len = 0, cap = max(cap, 4), reserved = 0.
    fn build_array_new_fn(
        context: &'ctx Context,
        module: &Module<'ctx>,
        malloc_fn: FunctionValue<'ctx>,
    ) -> FunctionValue<'ctx> {
        let i8_ptr = context.ptr_type(AddressSpace::default());
        let i64_type = context.i64_type();
        let fn_type = i8_ptr.fn_type(&[i64_type.into()], false);
        let func = module.add_function("modus_array_new", fn_type, None);

        let builder = context.create_builder();
        let entry_bb = context.append_basic_block(func, "entry");
        builder.position_at_end(entry_bb);

        let cap_arg = func.get_first_param().unwrap().into_int_value();
        let min_cap = i64_type.const_int(4, false);
        let cap_lt_4 = builder
            .build_int_compare(IntPredicate::SLT, cap_arg, min_cap, "cap_lt_4")
            .unwrap();
        let cap_val = builder
            .build_select(cap_lt_4, min_cap, cap_arg, "actual_cap")
            .unwrap()
            .into_int_value();

        // size = (4 + cap_val) * 8
        let words = builder
            .build_int_add(cap_val, i64_type.const_int(4, false), "words")
            .unwrap();
        let size_bytes = builder
            .build_int_mul(words, i64_type.const_int(8, false), "size_bytes")
            .unwrap();

        let mem_call = builder
            .build_call(malloc_fn, &[size_bytes.into()], "raw_mem")
            .unwrap();
        let mem_ptr = mem_call
            .try_as_basic_value()
            .basic()
            .unwrap()
            .into_pointer_value();

        // Store rc = 1 at offset 0
        let _ = builder.build_store(mem_ptr, i64_type.const_int(1, false));

        // Store len = 0 at offset 1
        let p1 = unsafe {
            builder
                .build_gep(
                    i64_type,
                    mem_ptr,
                    &[i64_type.const_int(1, false)],
                    "len_ptr",
                )
                .unwrap()
        };
        let _ = builder.build_store(p1, i64_type.const_int(0, false));

        // Store cap at offset 2
        let p2 = unsafe {
            builder
                .build_gep(
                    i64_type,
                    mem_ptr,
                    &[i64_type.const_int(2, false)],
                    "cap_ptr",
                )
                .unwrap()
        };
        let _ = builder.build_store(p2, cap_val);

        // Store 0 at offset 3
        let p3 = unsafe {
            builder
                .build_gep(
                    i64_type,
                    mem_ptr,
                    &[i64_type.const_int(3, false)],
                    "res_ptr",
                )
                .unwrap()
        };
        let _ = builder.build_store(p3, i64_type.const_int(0, false));

        let _ = builder.build_return(Some(&mem_ptr));
        func
    }

    /// Emits `modus_array_push(builder: ptr, elem: i64, elem_is_heap: i1) -> ptr`:
    /// Functional-But-In-Place (FBIP) accumulation.
    /// If rc == 1:
    ///   if len < cap: in-place write elem at 4 + len, len += 1, return builder
    ///   if len == cap: double cap, allocate new buffer, copy 4 header words + len elements, write elem, free(old), return new
    /// If rc > 1:
    ///   copy-on-write: allocate new buffer, copy elements (inc_ref if heap), write elem, dec_ref(old), return new
    fn build_array_push_fn(
        context: &'ctx Context,
        module: &Module<'ctx>,
        malloc_fn: FunctionValue<'ctx>,
        free_fn: FunctionValue<'ctx>,
        memcpy_fn: FunctionValue<'ctx>,
        inc_ref_fn: FunctionValue<'ctx>,
        dec_ref_fn: FunctionValue<'ctx>,
    ) -> FunctionValue<'ctx> {
        let i8_ptr = context.ptr_type(AddressSpace::default());
        let i64_type = context.i64_type();
        let i1_type = context.bool_type();
        let fn_type = i8_ptr.fn_type(&[i8_ptr.into(), i64_type.into(), i1_type.into()], false);
        let func = module.add_function("modus_array_push", fn_type, None);

        let builder = context.create_builder();
        let entry_bb = context.append_basic_block(func, "entry");
        let unique_bb = context.append_basic_block(func, "unique_path");
        let cow_bb = context.append_basic_block(func, "cow_path");

        // Unique sub-blocks
        let inplace_bb = context.append_basic_block(func, "unique_inplace");
        let grow_bb = context.append_basic_block(func, "unique_grow");

        builder.position_at_end(entry_bb);
        let builder_arg = func.get_nth_param(0).unwrap().into_pointer_value();
        let elem_arg = func.get_nth_param(1).unwrap().into_int_value();
        let elem_is_heap = func.get_nth_param(2).unwrap().into_int_value();

        let rc = builder
            .build_load(i64_type, builder_arg, "rc")
            .unwrap()
            .into_int_value();
        let is_unique = builder
            .build_int_compare(
                IntPredicate::EQ,
                rc,
                i64_type.const_int(1, false),
                "is_unique",
            )
            .unwrap();
        let _ = builder.build_conditional_branch(is_unique, unique_bb, cow_bb);

        // --- Unique path (rc == 1) ---
        builder.position_at_end(unique_bb);
        let len_ptr = unsafe {
            builder
                .build_gep(
                    i64_type,
                    builder_arg,
                    &[i64_type.const_int(1, false)],
                    "len_ptr",
                )
                .unwrap()
        };
        let len = builder
            .build_load(i64_type, len_ptr, "len")
            .unwrap()
            .into_int_value();
        let cap_ptr = unsafe {
            builder
                .build_gep(
                    i64_type,
                    builder_arg,
                    &[i64_type.const_int(2, false)],
                    "cap_ptr",
                )
                .unwrap()
        };
        let cap = builder
            .build_load(i64_type, cap_ptr, "cap")
            .unwrap()
            .into_int_value();

        let new_len = builder
            .build_int_add(len, i64_type.const_int(1, false), "new_len")
            .unwrap();
        let slot_offset = builder
            .build_int_add(len, i64_type.const_int(4, false), "slot_offset")
            .unwrap();

        let has_space = builder
            .build_int_compare(IntPredicate::SLT, len, cap, "has_space")
            .unwrap();
        let _ = builder.build_conditional_branch(has_space, inplace_bb, grow_bb);

        // Subpath: unique_inplace
        builder.position_at_end(inplace_bb);
        let elem_ptr = unsafe {
            builder
                .build_gep(i64_type, builder_arg, &[slot_offset], "elem_slot")
                .unwrap()
        };
        let _ = builder.build_store(elem_ptr, elem_arg);
        let _ = builder.build_store(len_ptr, new_len);
        let _ = builder.build_return(Some(&builder_arg));

        // Subpath: unique_grow (rc == 1, but len == cap)
        builder.position_at_end(grow_bb);
        let doubled_cap = builder
            .build_int_mul(cap, i64_type.const_int(2, false), "doubled_cap")
            .unwrap();
        let new_cap_lt_4 = builder
            .build_int_compare(
                IntPredicate::SLT,
                doubled_cap,
                i64_type.const_int(4, false),
                "new_cap_lt_4",
            )
            .unwrap();
        let new_cap = builder
            .build_select(
                new_cap_lt_4,
                i64_type.const_int(4, false),
                doubled_cap,
                "new_cap",
            )
            .unwrap()
            .into_int_value();
        let new_words = builder
            .build_int_add(new_cap, i64_type.const_int(4, false), "new_words")
            .unwrap();
        let new_size = builder
            .build_int_mul(new_words, i64_type.const_int(8, false), "new_size")
            .unwrap();
        let new_buf_call = builder
            .build_call(malloc_fn, &[new_size.into()], "new_buf")
            .unwrap();
        let new_buf = new_buf_call
            .try_as_basic_value()
            .basic()
            .unwrap()
            .into_pointer_value();

        // Copy existing elements + headers: (4 + len) * 8 bytes
        let copy_words = builder
            .build_int_add(len, i64_type.const_int(4, false), "copy_words")
            .unwrap();
        let copy_bytes = builder
            .build_int_mul(copy_words, i64_type.const_int(8, false), "copy_bytes")
            .unwrap();
        let _ = builder.build_call(
            memcpy_fn,
            &[new_buf.into(), builder_arg.into(), copy_bytes.into()],
            "",
        );

        // Update rc = 1, cap = new_cap, len = len + 1
        let _ = builder.build_store(new_buf, i64_type.const_int(1, false));
        let new_buf_len_ptr = unsafe {
            builder
                .build_gep(
                    i64_type,
                    new_buf,
                    &[i64_type.const_int(1, false)],
                    "nb_len_ptr",
                )
                .unwrap()
        };
        let _ = builder.build_store(new_buf_len_ptr, new_len);
        let new_buf_cap_ptr = unsafe {
            builder
                .build_gep(
                    i64_type,
                    new_buf,
                    &[i64_type.const_int(2, false)],
                    "nb_cap_ptr",
                )
                .unwrap()
        };
        let _ = builder.build_store(new_buf_cap_ptr, new_cap);

        // Store elem at new_buf + 4 + len
        let new_elem_ptr = unsafe {
            builder
                .build_gep(i64_type, new_buf, &[slot_offset], "new_elem_slot")
                .unwrap()
        };
        let _ = builder.build_store(new_elem_ptr, elem_arg);

        // Free old buffer since rc was 1
        let _ = builder.build_call(free_fn, &[builder_arg.into()], "");
        let _ = builder.build_return(Some(&new_buf));

        // --- COW path (rc > 1) ---
        builder.position_at_end(cow_bb);
        let cow_len_ptr = unsafe {
            builder
                .build_gep(
                    i64_type,
                    builder_arg,
                    &[i64_type.const_int(1, false)],
                    "c_len_ptr",
                )
                .unwrap()
        };
        let cow_len = builder
            .build_load(i64_type, cow_len_ptr, "c_len")
            .unwrap()
            .into_int_value();
        let cow_cap_ptr = unsafe {
            builder
                .build_gep(
                    i64_type,
                    builder_arg,
                    &[i64_type.const_int(2, false)],
                    "c_cap_ptr",
                )
                .unwrap()
        };
        let cow_cap = builder
            .build_load(i64_type, cow_cap_ptr, "c_cap")
            .unwrap()
            .into_int_value();

        let cow_needed = builder
            .build_int_add(cow_len, i64_type.const_int(1, false), "c_needed")
            .unwrap();
        let cow_cap_ok = builder
            .build_int_compare(IntPredicate::SGE, cow_cap, cow_needed, "c_cap_ok")
            .unwrap();
        let cow_double = builder
            .build_int_mul(cow_cap, i64_type.const_int(2, false), "c_double")
            .unwrap();
        let cow_target_cap = builder
            .build_select(cow_cap_ok, cow_cap, cow_double, "c_target_cap")
            .unwrap()
            .into_int_value();
        let cow_cap_lt_4 = builder
            .build_int_compare(
                IntPredicate::SLT,
                cow_target_cap,
                i64_type.const_int(4, false),
                "c_lt_4",
            )
            .unwrap();
        let cow_actual_cap = builder
            .build_select(
                cow_cap_lt_4,
                i64_type.const_int(4, false),
                cow_target_cap,
                "c_actual_cap",
            )
            .unwrap()
            .into_int_value();

        let cow_words = builder
            .build_int_add(cow_actual_cap, i64_type.const_int(4, false), "c_words")
            .unwrap();
        let cow_size = builder
            .build_int_mul(cow_words, i64_type.const_int(8, false), "c_size")
            .unwrap();
        let cow_buf_call = builder
            .build_call(malloc_fn, &[cow_size.into()], "c_buf")
            .unwrap();
        let cow_buf = cow_buf_call
            .try_as_basic_value()
            .basic()
            .unwrap()
            .into_pointer_value();

        // Copy elements
        let cow_copy_words = builder
            .build_int_add(cow_len, i64_type.const_int(4, false), "cc_words")
            .unwrap();
        let cow_copy_bytes = builder
            .build_int_mul(cow_copy_words, i64_type.const_int(8, false), "cc_bytes")
            .unwrap();
        let _ = builder.build_call(
            memcpy_fn,
            &[cow_buf.into(), builder_arg.into(), cow_copy_bytes.into()],
            "",
        );

        // Header: rc = 1, len = cow_len + 1, cap = cow_actual_cap
        let _ = builder.build_store(cow_buf, i64_type.const_int(1, false));
        let cow_buf_len_ptr = unsafe {
            builder
                .build_gep(
                    i64_type,
                    cow_buf,
                    &[i64_type.const_int(1, false)],
                    "cbl_ptr",
                )
                .unwrap()
        };
        let cow_new_len = builder
            .build_int_add(cow_len, i64_type.const_int(1, false), "cn_len")
            .unwrap();
        let _ = builder.build_store(cow_buf_len_ptr, cow_new_len);
        let cow_buf_cap_ptr = unsafe {
            builder
                .build_gep(
                    i64_type,
                    cow_buf,
                    &[i64_type.const_int(2, false)],
                    "cbc_ptr",
                )
                .unwrap()
        };
        let _ = builder.build_store(cow_buf_cap_ptr, cow_actual_cap);

        // If heap elements, inc_ref existing elements [0..cow_len]
        let cow_inc_loop_bb = context.append_basic_block(func, "cow_inc_loop");
        let cow_inc_body_bb = context.append_basic_block(func, "cow_inc_body");
        let cow_finish_bb = context.append_basic_block(func, "cow_finish");

        let cow_has_heap_elems = builder
            .build_int_compare(
                IntPredicate::NE,
                elem_is_heap,
                context.bool_type().const_int(0, false),
                "has_heap",
            )
            .unwrap();
        let cow_has_items = builder
            .build_int_compare(
                IntPredicate::SGT,
                cow_len,
                i64_type.const_int(0, false),
                "has_items",
            )
            .unwrap();
        let needs_inc = builder
            .build_and(cow_has_heap_elems, cow_has_items, "needs_inc")
            .unwrap();

        let cow_idx_alloca = builder.build_alloca(i64_type, "cow_idx").unwrap();
        let _ = builder.build_store(cow_idx_alloca, i64_type.const_int(0, false));
        let _ = builder.build_conditional_branch(needs_inc, cow_inc_loop_bb, cow_finish_bb);

        builder.position_at_end(cow_inc_loop_bb);
        let cur_idx = builder
            .build_load(i64_type, cow_idx_alloca, "cur_idx")
            .unwrap()
            .into_int_value();
        let in_bounds = builder
            .build_int_compare(IntPredicate::SLT, cur_idx, cow_len, "in_bounds")
            .unwrap();
        let _ = builder.build_conditional_branch(in_bounds, cow_inc_body_bb, cow_finish_bb);

        builder.position_at_end(cow_inc_body_bb);
        let cur_offset = builder
            .build_int_add(cur_idx, i64_type.const_int(4, false), "cur_off")
            .unwrap();
        let cur_elem_slot = unsafe {
            builder
                .build_gep(i64_type, cow_buf, &[cur_offset], "c_elem_slot")
                .unwrap()
        };
        let cur_elem_int = builder
            .build_load(i64_type, cur_elem_slot, "c_el_int")
            .unwrap()
            .into_int_value();
        let cur_elem_ptr = builder
            .build_int_to_ptr(cur_elem_int, i8_ptr, "c_el_ptr")
            .unwrap();
        let _ = builder.build_call(inc_ref_fn, &[cur_elem_ptr.into()], "");
        let next_idx = builder
            .build_int_add(cur_idx, i64_type.const_int(1, false), "next_idx")
            .unwrap();
        let _ = builder.build_store(cow_idx_alloca, next_idx);
        let _ = builder.build_unconditional_branch(cow_inc_loop_bb);

        builder.position_at_end(cow_finish_bb);
        // Store new element at 4 + cow_len
        let cow_slot = builder
            .build_int_add(cow_len, i64_type.const_int(4, false), "c_slot")
            .unwrap();
        let cow_dest = unsafe {
            builder
                .build_gep(i64_type, cow_buf, &[cow_slot], "c_dest")
                .unwrap()
        };
        let _ = builder.build_store(cow_dest, elem_arg);

        // Decrement RC of old builder
        let _ = builder.build_call(dec_ref_fn, &[builder_arg.into()], "");
        let _ = builder.build_return(Some(&cow_buf));

        func
    }

    /// Emits `modus_array_build(builder: ptr, elem_is_heap: i1) -> ptr`:
    /// Finalizes builder into immutable array [T].
    /// If rc == 1: Zero-copy! Returns builder directly.
    /// If rc > 1: Clones exact-sized array [T], inc_refs elements if heap, dec_refs old builder, returns new array.
    fn build_array_build_fn(
        context: &'ctx Context,
        module: &Module<'ctx>,
        malloc_fn: FunctionValue<'ctx>,
        memcpy_fn: FunctionValue<'ctx>,
        inc_ref_fn: FunctionValue<'ctx>,
        dec_ref_fn: FunctionValue<'ctx>,
    ) -> FunctionValue<'ctx> {
        let i8_ptr = context.ptr_type(AddressSpace::default());
        let i64_type = context.i64_type();
        let i1_type = context.bool_type();
        let fn_type = i8_ptr.fn_type(&[i8_ptr.into(), i1_type.into()], false);
        let func = module.add_function("modus_array_build", fn_type, None);

        let builder = context.create_builder();
        let entry_bb = context.append_basic_block(func, "entry");
        let unique_bb = context.append_basic_block(func, "unique_build");
        let cow_bb = context.append_basic_block(func, "cow_build");

        builder.position_at_end(entry_bb);
        let builder_arg = func.get_nth_param(0).unwrap().into_pointer_value();
        let elem_is_heap = func.get_nth_param(1).unwrap().into_int_value();

        let rc = builder
            .build_load(i64_type, builder_arg, "rc")
            .unwrap()
            .into_int_value();
        let is_unique = builder
            .build_int_compare(
                IntPredicate::EQ,
                rc,
                i64_type.const_int(1, false),
                "is_unique",
            )
            .unwrap();
        let _ = builder.build_conditional_branch(is_unique, unique_bb, cow_bb);

        // --- Unique path: zero-copy! ---
        builder.position_at_end(unique_bb);
        let _ = builder.build_return(Some(&builder_arg));

        // --- COW path: copy elements into fresh [T] ---
        builder.position_at_end(cow_bb);
        let len_ptr = unsafe {
            builder
                .build_gep(
                    i64_type,
                    builder_arg,
                    &[i64_type.const_int(1, false)],
                    "len_ptr",
                )
                .unwrap()
        };
        let len = builder
            .build_load(i64_type, len_ptr, "len")
            .unwrap()
            .into_int_value();
        let words = builder
            .build_int_add(len, i64_type.const_int(4, false), "words")
            .unwrap();
        let size_bytes = builder
            .build_int_mul(words, i64_type.const_int(8, false), "size_bytes")
            .unwrap();
        let new_call = builder
            .build_call(malloc_fn, &[size_bytes.into()], "new_arr")
            .unwrap();
        let new_arr = new_call
            .try_as_basic_value()
            .basic()
            .unwrap()
            .into_pointer_value();

        let _ = builder.build_call(
            memcpy_fn,
            &[new_arr.into(), builder_arg.into(), size_bytes.into()],
            "",
        );
        // Store rc = 1, cap = len
        let _ = builder.build_store(new_arr, i64_type.const_int(1, false));
        let new_cap_ptr = unsafe {
            builder
                .build_gep(
                    i64_type,
                    new_arr,
                    &[i64_type.const_int(2, false)],
                    "new_cap_ptr",
                )
                .unwrap()
        };
        let _ = builder.build_store(new_cap_ptr, len);

        // If heap elements, inc_ref each copied element
        let inc_loop_bb = context.append_basic_block(func, "inc_loop");
        let inc_body_bb = context.append_basic_block(func, "inc_body");
        let finish_bb = context.append_basic_block(func, "finish");

        let has_heap = builder
            .build_int_compare(
                IntPredicate::NE,
                elem_is_heap,
                context.bool_type().const_int(0, false),
                "has_heap",
            )
            .unwrap();
        let has_items = builder
            .build_int_compare(
                IntPredicate::SGT,
                len,
                i64_type.const_int(0, false),
                "has_items",
            )
            .unwrap();
        let needs_inc = builder.build_and(has_heap, has_items, "needs_inc").unwrap();

        let idx_alloca = builder.build_alloca(i64_type, "b_idx").unwrap();
        let _ = builder.build_store(idx_alloca, i64_type.const_int(0, false));
        let _ = builder.build_conditional_branch(needs_inc, inc_loop_bb, finish_bb);

        builder.position_at_end(inc_loop_bb);
        let idx = builder
            .build_load(i64_type, idx_alloca, "idx")
            .unwrap()
            .into_int_value();
        let in_bounds = builder
            .build_int_compare(IntPredicate::SLT, idx, len, "in_bounds")
            .unwrap();
        let _ = builder.build_conditional_branch(in_bounds, inc_body_bb, finish_bb);

        builder.position_at_end(inc_body_bb);
        let offset = builder
            .build_int_add(idx, i64_type.const_int(4, false), "off")
            .unwrap();
        let elem_slot = unsafe {
            builder
                .build_gep(i64_type, new_arr, &[offset], "el_slot")
                .unwrap()
        };
        let elem_int = builder
            .build_load(i64_type, elem_slot, "el_int")
            .unwrap()
            .into_int_value();
        let elem_ptr = builder
            .build_int_to_ptr(elem_int, i8_ptr, "el_ptr")
            .unwrap();
        let _ = builder.build_call(inc_ref_fn, &[elem_ptr.into()], "");
        let next_idx = builder
            .build_int_add(idx, i64_type.const_int(1, false), "next_idx")
            .unwrap();
        let _ = builder.build_store(idx_alloca, next_idx);
        let _ = builder.build_unconditional_branch(inc_loop_bb);

        builder.position_at_end(finish_bb);
        let _ = builder.build_call(dec_ref_fn, &[builder_arg.into()], "");
        let _ = builder.build_return(Some(&new_arr));

        func
    }

    /// Emits `modus_array_set(arr: ptr, idx: i64, elem: i64, elem_is_heap: i1) -> ptr`:
    /// Functional-But-In-Place (FBIP) array indexed update.
    /// If idx < 0 || idx >= len: return arr (bounds check safety).
    /// If rc == 1:
    ///   if elem_is_heap: dec_ref old element at arr[4 + idx]
    ///   store elem at arr[4 + idx]
    ///   return arr
    /// If rc > 1:
    ///   allocate new buffer of (4 + cap) words
    ///   memcpy (4 + len) words from arr to new_buf
    ///   set rc = 1 in new_buf
    ///   if elem_is_heap: inc_ref elements at all i != idx
    ///   store elem at new_buf[4 + idx]
    ///   dec_ref(arr)
    ///   return new_buf
    fn build_array_set_fn(
        context: &'ctx Context,
        module: &Module<'ctx>,
        malloc_fn: FunctionValue<'ctx>,
        memcpy_fn: FunctionValue<'ctx>,
        inc_ref_fn: FunctionValue<'ctx>,
        dec_ref_fn: FunctionValue<'ctx>,
    ) -> FunctionValue<'ctx> {
        let i8_ptr = context.ptr_type(AddressSpace::default());
        let i64_type = context.i64_type();
        let i1_type = context.bool_type();
        let fn_type = i8_ptr.fn_type(
            &[
                i8_ptr.into(),
                i64_type.into(),
                i64_type.into(),
                i1_type.into(),
            ],
            false,
        );
        let func = module.add_function("modus_array_set", fn_type, None);

        let builder = context.create_builder();
        let entry_bb = context.append_basic_block(func, "entry");
        let bounds_ok_bb = context.append_basic_block(func, "bounds_ok");
        let unique_bb = context.append_basic_block(func, "unique_path");
        let cow_bb = context.append_basic_block(func, "cow_path");
        let ret_early_bb = context.append_basic_block(func, "ret_early");

        builder.position_at_end(entry_bb);
        let arr_arg = func.get_nth_param(0).unwrap().into_pointer_value();
        let idx_arg = func.get_nth_param(1).unwrap().into_int_value();
        let elem_arg = func.get_nth_param(2).unwrap().into_int_value();
        let elem_is_heap = func.get_nth_param(3).unwrap().into_int_value();

        let len_ptr = unsafe {
            builder
                .build_gep(
                    i64_type,
                    arr_arg,
                    &[i64_type.const_int(1, false)],
                    "len_ptr",
                )
                .unwrap()
        };
        let len = builder
            .build_load(i64_type, len_ptr, "len")
            .unwrap()
            .into_int_value();

        let ge_zero = builder
            .build_int_compare(
                IntPredicate::SGE,
                idx_arg,
                i64_type.const_int(0, false),
                "ge_zero",
            )
            .unwrap();
        let lt_len = builder
            .build_int_compare(IntPredicate::SLT, idx_arg, len, "lt_len")
            .unwrap();
        let in_bounds = builder.build_and(ge_zero, lt_len, "in_bounds").unwrap();
        let _ = builder.build_conditional_branch(in_bounds, bounds_ok_bb, ret_early_bb);

        // --- In bounds ---
        builder.position_at_end(bounds_ok_bb);
        let rc = builder
            .build_load(i64_type, arr_arg, "rc")
            .unwrap()
            .into_int_value();
        let is_unique = builder
            .build_int_compare(
                IntPredicate::EQ,
                rc,
                i64_type.const_int(1, false),
                "is_unique",
            )
            .unwrap();
        let _ = builder.build_conditional_branch(is_unique, unique_bb, cow_bb);

        // --- Unique path (rc == 1) ---
        let unique_dec_bb = context.append_basic_block(func, "unique_dec");
        let unique_write_bb = context.append_basic_block(func, "unique_write");

        builder.position_at_end(unique_bb);
        let slot_offset = builder
            .build_int_add(idx_arg, i64_type.const_int(4, false), "slot_offset")
            .unwrap();
        let elem_ptr = unsafe {
            builder
                .build_gep(i64_type, arr_arg, &[slot_offset], "elem_slot")
                .unwrap()
        };
        let is_heap_cond = builder
            .build_int_compare(
                IntPredicate::NE,
                elem_is_heap,
                context.bool_type().const_int(0, false),
                "is_heap_cond",
            )
            .unwrap();
        let _ = builder.build_conditional_branch(is_heap_cond, unique_dec_bb, unique_write_bb);

        builder.position_at_end(unique_dec_bb);
        let old_elem_int = builder
            .build_load(i64_type, elem_ptr, "old_el_int")
            .unwrap()
            .into_int_value();
        let old_elem_ptr = builder
            .build_int_to_ptr(old_elem_int, i8_ptr, "old_el_ptr")
            .unwrap();
        let _ = builder.build_call(dec_ref_fn, &[old_elem_ptr.into()], "");
        let _ = builder.build_unconditional_branch(unique_write_bb);

        builder.position_at_end(unique_write_bb);
        let _ = builder.build_store(elem_ptr, elem_arg);
        let _ = builder.build_return(Some(&arr_arg));

        // --- COW path (rc > 1) ---
        let cow_inc_loop_bb = context.append_basic_block(func, "cow_inc_loop");
        let cow_inc_check_bb = context.append_basic_block(func, "cow_inc_check");
        let cow_inc_body_bb = context.append_basic_block(func, "cow_inc_body");
        let cow_inc_next_bb = context.append_basic_block(func, "cow_inc_next");
        let cow_finish_bb = context.append_basic_block(func, "cow_finish");

        builder.position_at_end(cow_bb);
        let cap_ptr = unsafe {
            builder
                .build_gep(
                    i64_type,
                    arr_arg,
                    &[i64_type.const_int(2, false)],
                    "cap_ptr",
                )
                .unwrap()
        };
        let cap = builder
            .build_load(i64_type, cap_ptr, "cap")
            .unwrap()
            .into_int_value();

        let cow_words = builder
            .build_int_add(cap, i64_type.const_int(4, false), "c_words")
            .unwrap();
        let cow_size = builder
            .build_int_mul(cow_words, i64_type.const_int(8, false), "c_size")
            .unwrap();
        let cow_buf_call = builder
            .build_call(malloc_fn, &[cow_size.into()], "c_buf")
            .unwrap();
        let cow_buf = cow_buf_call
            .try_as_basic_value()
            .basic()
            .unwrap()
            .into_pointer_value();

        // Copy elements + headers: (4 + len) * 8
        let copy_words = builder
            .build_int_add(len, i64_type.const_int(4, false), "copy_words")
            .unwrap();
        let copy_bytes = builder
            .build_int_mul(copy_words, i64_type.const_int(8, false), "copy_bytes")
            .unwrap();
        let _ = builder.build_call(
            memcpy_fn,
            &[cow_buf.into(), arr_arg.into(), copy_bytes.into()],
            "",
        );

        // Header: rc = 1
        let _ = builder.build_store(cow_buf, i64_type.const_int(1, false));

        let has_heap_elems = builder
            .build_int_compare(
                IntPredicate::NE,
                elem_is_heap,
                context.bool_type().const_int(0, false),
                "has_heap",
            )
            .unwrap();
        let has_items = builder
            .build_int_compare(
                IntPredicate::SGT,
                len,
                i64_type.const_int(0, false),
                "has_items",
            )
            .unwrap();
        let needs_inc = builder
            .build_and(has_heap_elems, has_items, "needs_inc")
            .unwrap();

        let cow_idx_alloca = builder.build_alloca(i64_type, "cow_idx").unwrap();
        let _ = builder.build_store(cow_idx_alloca, i64_type.const_int(0, false));
        let _ = builder.build_conditional_branch(needs_inc, cow_inc_loop_bb, cow_finish_bb);

        // Loop i from 0 to len - 1: if i != idx { inc_ref(elem) }
        builder.position_at_end(cow_inc_loop_bb);
        let cur_i = builder
            .build_load(i64_type, cow_idx_alloca, "cur_i")
            .unwrap()
            .into_int_value();
        let in_loop = builder
            .build_int_compare(IntPredicate::SLT, cur_i, len, "in_loop")
            .unwrap();
        let _ = builder.build_conditional_branch(in_loop, cow_inc_check_bb, cow_finish_bb);

        builder.position_at_end(cow_inc_check_bb);
        let is_target = builder
            .build_int_compare(IntPredicate::EQ, cur_i, idx_arg, "is_target")
            .unwrap();
        let _ = builder.build_conditional_branch(is_target, cow_inc_next_bb, cow_inc_body_bb);

        builder.position_at_end(cow_inc_body_bb);
        let cur_slot = builder
            .build_int_add(cur_i, i64_type.const_int(4, false), "c_slot")
            .unwrap();
        let cur_slot_ptr = unsafe {
            builder
                .build_gep(i64_type, cow_buf, &[cur_slot], "c_slot_ptr")
                .unwrap()
        };
        let cur_elem_int = builder
            .build_load(i64_type, cur_slot_ptr, "c_elem_int")
            .unwrap()
            .into_int_value();
        let cur_elem_ptr = builder
            .build_int_to_ptr(cur_elem_int, i8_ptr, "c_elem_ptr")
            .unwrap();
        let _ = builder.build_call(inc_ref_fn, &[cur_elem_ptr.into()], "");
        let _ = builder.build_unconditional_branch(cow_inc_next_bb);

        builder.position_at_end(cow_inc_next_bb);
        let next_i = builder
            .build_int_add(cur_i, i64_type.const_int(1, false), "next_i")
            .unwrap();
        let _ = builder.build_store(cow_idx_alloca, next_i);
        let _ = builder.build_unconditional_branch(cow_inc_loop_bb);

        builder.position_at_end(cow_finish_bb);
        let target_slot = builder
            .build_int_add(idx_arg, i64_type.const_int(4, false), "t_slot")
            .unwrap();
        let target_ptr = unsafe {
            builder
                .build_gep(i64_type, cow_buf, &[target_slot], "t_slot_ptr")
                .unwrap()
        };
        let _ = builder.build_store(target_ptr, elem_arg);
        let _ = builder.build_call(dec_ref_fn, &[arr_arg.into()], "");
        let _ = builder.build_return(Some(&cow_buf));

        // --- Ret early (bounds fail) ---
        builder.position_at_end(ret_early_bb);
        let _ = builder.build_return(Some(&arr_arg));

        func
    }

    /// Emits `modus_array_pop(arr: ptr, elem_is_heap: i1) -> ptr`:
    /// Functional-But-In-Place (FBIP) array pop.
    /// If len == 0: returns arr unmodified.
    /// If rc == 1:
    ///   new_len = len - 1
    ///   if elem_is_heap: dec_ref element at arr[4 + new_len]
    ///   store new_len at arr[1]
    ///   return arr
    /// If rc > 1:
    ///   new_len = len - 1
    ///   allocate new buffer of (4 + cap) words
    ///   memcpy (4 + new_len) words from arr to new_buf
    ///   store 1 at new_buf[0] (rc)
    ///   store new_len at new_buf[1] (len)
    ///   store cap at new_buf[2] (cap)
    ///   store 0 at new_buf[3] (res)
    ///   if elem_is_heap: inc_ref elements [0..new_len)
    ///   dec_ref(arr)
    ///   return new_buf
    fn build_array_pop_fn(
        context: &'ctx Context,
        module: &Module<'ctx>,
        malloc_fn: FunctionValue<'ctx>,
        memcpy_fn: FunctionValue<'ctx>,
        inc_ref_fn: FunctionValue<'ctx>,
        dec_ref_fn: FunctionValue<'ctx>,
    ) -> FunctionValue<'ctx> {
        let i8_ptr = context.ptr_type(AddressSpace::default());
        let i64_type = context.i64_type();
        let i1_type = context.bool_type();
        let fn_type = i8_ptr.fn_type(&[i8_ptr.into(), i1_type.into()], false);
        let func = module.add_function("modus_array_pop", fn_type, None);

        let builder = context.create_builder();
        let entry_bb = context.append_basic_block(func, "entry");
        let has_elems_bb = context.append_basic_block(func, "has_elems");
        let unique_bb = context.append_basic_block(func, "unique_pop");
        let cow_bb = context.append_basic_block(func, "cow_pop");
        let ret_early_bb = context.append_basic_block(func, "ret_early");

        builder.position_at_end(entry_bb);
        let arr_arg = func.get_nth_param(0).unwrap().into_pointer_value();
        let elem_is_heap = func.get_nth_param(1).unwrap().into_int_value();

        let len_ptr = unsafe {
            builder
                .build_gep(
                    i64_type,
                    arr_arg,
                    &[i64_type.const_int(1, false)],
                    "len_ptr",
                )
                .unwrap()
        };
        let len = builder
            .build_load(i64_type, len_ptr, "len")
            .unwrap()
            .into_int_value();

        let has_elems = builder
            .build_int_compare(
                IntPredicate::SGT,
                len,
                i64_type.const_int(0, false),
                "has_elems",
            )
            .unwrap();
        let _ = builder.build_conditional_branch(has_elems, has_elems_bb, ret_early_bb);

        builder.position_at_end(has_elems_bb);
        let new_len = builder
            .build_int_sub(len, i64_type.const_int(1, false), "new_len")
            .unwrap();
        let rc = builder
            .build_load(i64_type, arr_arg, "rc")
            .unwrap()
            .into_int_value();
        let is_unique = builder
            .build_int_compare(
                IntPredicate::EQ,
                rc,
                i64_type.const_int(1, false),
                "is_unique",
            )
            .unwrap();
        let _ = builder.build_conditional_branch(is_unique, unique_bb, cow_bb);

        // --- Unique path (rc == 1) ---
        let unique_dec_bb = context.append_basic_block(func, "unique_dec");
        let unique_store_bb = context.append_basic_block(func, "unique_store");

        builder.position_at_end(unique_bb);
        let is_heap_cond = builder
            .build_int_compare(
                IntPredicate::NE,
                elem_is_heap,
                context.bool_type().const_int(0, false),
                "is_heap_cond",
            )
            .unwrap();
        let _ = builder.build_conditional_branch(is_heap_cond, unique_dec_bb, unique_store_bb);

        builder.position_at_end(unique_dec_bb);
        let last_slot = builder
            .build_int_add(new_len, i64_type.const_int(4, false), "last_slot")
            .unwrap();
        let last_elem_ptr = unsafe {
            builder
                .build_gep(i64_type, arr_arg, &[last_slot], "last_elem_ptr")
                .unwrap()
        };
        let last_elem_int = builder
            .build_load(i64_type, last_elem_ptr, "last_elem_int")
            .unwrap()
            .into_int_value();
        let last_elem_p = builder
            .build_int_to_ptr(last_elem_int, i8_ptr, "last_elem_p")
            .unwrap();
        let _ = builder.build_call(dec_ref_fn, &[last_elem_p.into()], "");
        let _ = builder.build_unconditional_branch(unique_store_bb);

        builder.position_at_end(unique_store_bb);
        let _ = builder.build_store(len_ptr, new_len);
        let _ = builder.build_return(Some(&arr_arg));

        // --- COW path (rc > 1) ---
        let cow_inc_loop_bb = context.append_basic_block(func, "cow_inc_loop");
        let cow_inc_body_bb = context.append_basic_block(func, "cow_inc_body");
        let cow_finish_bb = context.append_basic_block(func, "cow_finish");

        builder.position_at_end(cow_bb);
        let cap_ptr = unsafe {
            builder
                .build_gep(
                    i64_type,
                    arr_arg,
                    &[i64_type.const_int(2, false)],
                    "cap_ptr",
                )
                .unwrap()
        };
        let cap = builder
            .build_load(i64_type, cap_ptr, "cap")
            .unwrap()
            .into_int_value();

        let cow_words = builder
            .build_int_add(cap, i64_type.const_int(4, false), "c_words")
            .unwrap();
        let cow_size = builder
            .build_int_mul(cow_words, i64_type.const_int(8, false), "c_size")
            .unwrap();
        let cow_buf_call = builder
            .build_call(malloc_fn, &[cow_size.into()], "c_buf")
            .unwrap();
        let cow_buf = cow_buf_call
            .try_as_basic_value()
            .basic()
            .unwrap()
            .into_pointer_value();

        // Copy elements + headers up to new_len: (4 + new_len) * 8
        let copy_words = builder
            .build_int_add(new_len, i64_type.const_int(4, false), "copy_words")
            .unwrap();
        let copy_bytes = builder
            .build_int_mul(copy_words, i64_type.const_int(8, false), "copy_bytes")
            .unwrap();
        let _ = builder.build_call(
            memcpy_fn,
            &[cow_buf.into(), arr_arg.into(), copy_bytes.into()],
            "",
        );

        // Header: rc = 1, len = new_len, cap = cap, res = 0
        let _ = builder.build_store(cow_buf, i64_type.const_int(1, false));
        let nb_len_ptr = unsafe {
            builder
                .build_gep(
                    i64_type,
                    cow_buf,
                    &[i64_type.const_int(1, false)],
                    "nb_len_ptr",
                )
                .unwrap()
        };
        let _ = builder.build_store(nb_len_ptr, new_len);
        let nb_cap_ptr = unsafe {
            builder
                .build_gep(
                    i64_type,
                    cow_buf,
                    &[i64_type.const_int(2, false)],
                    "nb_cap_ptr",
                )
                .unwrap()
        };
        let _ = builder.build_store(nb_cap_ptr, cap);
        let nb_res_ptr = unsafe {
            builder
                .build_gep(
                    i64_type,
                    cow_buf,
                    &[i64_type.const_int(3, false)],
                    "nb_res_ptr",
                )
                .unwrap()
        };
        let _ = builder.build_store(nb_res_ptr, i64_type.const_int(0, false));

        let has_heap_elems = builder
            .build_int_compare(
                IntPredicate::NE,
                elem_is_heap,
                context.bool_type().const_int(0, false),
                "has_heap",
            )
            .unwrap();
        let has_retained = builder
            .build_int_compare(
                IntPredicate::SGT,
                new_len,
                i64_type.const_int(0, false),
                "has_retained",
            )
            .unwrap();
        let needs_inc = builder
            .build_and(has_heap_elems, has_retained, "needs_inc")
            .unwrap();

        let cow_idx_alloca = builder.build_alloca(i64_type, "cow_idx").unwrap();
        let _ = builder.build_store(cow_idx_alloca, i64_type.const_int(0, false));
        let _ = builder.build_conditional_branch(needs_inc, cow_inc_loop_bb, cow_finish_bb);

        builder.position_at_end(cow_inc_loop_bb);
        let cur_idx = builder
            .build_load(i64_type, cow_idx_alloca, "cur_idx")
            .unwrap()
            .into_int_value();
        let in_bounds = builder
            .build_int_compare(IntPredicate::SLT, cur_idx, new_len, "in_bounds")
            .unwrap();
        let _ = builder.build_conditional_branch(in_bounds, cow_inc_body_bb, cow_finish_bb);

        builder.position_at_end(cow_inc_body_bb);
        let cur_offset = builder
            .build_int_add(cur_idx, i64_type.const_int(4, false), "cur_off")
            .unwrap();
        let cur_elem_slot = unsafe {
            builder
                .build_gep(i64_type, cow_buf, &[cur_offset], "c_elem_slot")
                .unwrap()
        };
        let cur_elem_int = builder
            .build_load(i64_type, cur_elem_slot, "c_el_int")
            .unwrap()
            .into_int_value();
        let cur_elem_ptr = builder
            .build_int_to_ptr(cur_elem_int, i8_ptr, "c_el_ptr")
            .unwrap();
        let _ = builder.build_call(inc_ref_fn, &[cur_elem_ptr.into()], "");
        let next_idx = builder
            .build_int_add(cur_idx, i64_type.const_int(1, false), "next_idx")
            .unwrap();
        let _ = builder.build_store(cow_idx_alloca, next_idx);
        let _ = builder.build_unconditional_branch(cow_inc_loop_bb);

        builder.position_at_end(cow_finish_bb);
        let _ = builder.build_call(dec_ref_fn, &[arr_arg.into()], "");
        let _ = builder.build_return(Some(&cow_buf));

        // --- Ret early ---
        builder.position_at_end(ret_early_bb);
        let _ = builder.build_return(Some(&arr_arg));

        func
    }
}
