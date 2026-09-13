//! Runtime support, memory management primitives, and IO stubs for LLVM backend.
//!
//! Implements:
//! - `malloc` and `free` declarations
//! - `modus_alloc`: allocates heap object and initializes `rc = 1`
//! - `modus_inc_ref`: non-atomic reference count increment
//! - `modus_dec_ref`: inline fast-path (`sub` + `icmp eq 0` -> `free`)
//! - `modus_is_unique`: FBIP uniqueness check (`rc == 1`)
//! - IO stubs (`puts`, `printf`, `io_pure`)

use inkwell::AddressSpace;
use inkwell::IntPredicate;
use inkwell::context::Context;
use inkwell::module::Module;
use inkwell::values::{BasicValue, FunctionValue};

/// Manages runtime declarations and helper functions in an LLVM module.
pub struct Runtime<'ctx> {
    pub context: &'ctx Context,
    pub malloc_fn: FunctionValue<'ctx>,
    pub free_fn: FunctionValue<'ctx>,
    pub puts_fn: FunctionValue<'ctx>,
    pub printf_fn: FunctionValue<'ctx>,
    pub strlen_fn: FunctionValue<'ctx>,
    pub memcpy_fn: FunctionValue<'ctx>,
    pub snprintf_fn: FunctionValue<'ctx>,
    pub alloc_fn: FunctionValue<'ctx>,
    pub inc_ref_fn: FunctionValue<'ctx>,
    pub dec_ref_fn: FunctionValue<'ctx>,
    pub is_unique_fn: FunctionValue<'ctx>,
    pub str_concat_fn: FunctionValue<'ctx>,
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

        // 3. extern int puts(const char* str);
        let puts_fn = module.get_function("puts").unwrap_or_else(|| {
            let fn_type = i32_type.fn_type(&[i8_ptr.into()], false);
            module.add_function("puts", fn_type, None)
        });

        // 4. extern int printf(const char* format, ...);
        let printf_fn = module.get_function("printf").unwrap_or_else(|| {
            let fn_type = i32_type.fn_type(&[i8_ptr.into()], true);
            module.add_function("printf", fn_type, None)
        });

        // 5. extern size_t strlen(const char* str);
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

        // 12. Build helper: modus_str_concat(s1: ptr, s2: ptr) -> ptr
        let str_concat_fn = module.get_function("modus_str_concat").unwrap_or_else(|| {
            Self::build_str_concat_fn(context, module, malloc_fn, strlen_fn, memcpy_fn)
        });

        Self {
            context,
            malloc_fn,
            free_fn,
            puts_fn,
            printf_fn,
            strlen_fn,
            memcpy_fn,
            snprintf_fn,
            alloc_fn,
            inc_ref_fn,
            dec_ref_fn,
            is_unique_fn,
            str_concat_fn,
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
    fn build_inc_ref_fn(context: &'ctx Context, module: &Module<'ctx>) -> FunctionValue<'ctx> {
        let i8_ptr = context.ptr_type(AddressSpace::default());
        let i64_type = context.i64_type();
        let void_type = context.void_type();
        let fn_type = void_type.fn_type(&[i8_ptr.into()], false);
        let func = module.add_function("modus_inc_ref", fn_type, None);

        let builder = context.create_builder();
        let entry_bb = context.append_basic_block(func, "entry");
        builder.position_at_end(entry_bb);

        let ptr_arg = func.get_first_param().unwrap().into_pointer_value();
        if let Ok(val) = builder.build_load(i64_type, ptr_arg, "rc") {
            let rc = val.into_int_value();
            if let Ok(rc_plus_1) = builder.build_int_add(rc, i64_type.const_int(1, false), "rc_inc")
            {
                let _ = builder.build_store(ptr_arg, rc_plus_1);
            }
        }

        let _ = builder.build_return(None);
        func
    }

    /// Emits `modus_dec_ref(ptr: ptr) -> void`:
    /// Inline fast path: `sub` + `icmp eq 0` -> `free(ptr)`.
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
        let free_bb = context.append_basic_block(func, "free_block");
        let cont_bb = context.append_basic_block(func, "cont_block");

        builder.position_at_end(entry_bb);
        let ptr_arg = func.get_first_param().unwrap().into_pointer_value();

        // Null check: if ptr is null, return immediately
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

        let not_null_bb = context.append_basic_block(func, "not_null");
        let _ = builder.build_conditional_branch(is_null, cont_bb, not_null_bb);

        builder.position_at_end(not_null_bb);
        let rc = builder
            .build_load(i64_type, ptr_arg, "rc")
            .unwrap()
            .into_int_value();
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
        let _ = builder.build_conditional_branch(is_zero, free_bb, cont_bb);

        // Free block: calls free(ptr)
        builder.position_at_end(free_bb);
        let _ = builder.build_call(free_fn, &[ptr_arg.into()], "");
        let _ = builder.build_unconditional_branch(cont_bb);

        // Cont block: return
        builder.position_at_end(cont_bb);
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

    /// Emits `modus_str_concat(s1: ptr, s2: ptr) -> ptr`:
    /// Allocates buffer, copies s1 and s2, appends null terminator, and returns pointer.
    fn build_str_concat_fn(
        context: &'ctx Context,
        module: &Module<'ctx>,
        malloc_fn: FunctionValue<'ctx>,
        strlen_fn: FunctionValue<'ctx>,
        memcpy_fn: FunctionValue<'ctx>,
    ) -> FunctionValue<'ctx> {
        let i8_ptr = context.ptr_type(AddressSpace::default());
        let i64_type = context.i64_type();
        let fn_type = i8_ptr.fn_type(&[i8_ptr.into(), i8_ptr.into()], false);
        let func = module.add_function("modus_str_concat", fn_type, None);

        let builder = context.create_builder();
        let entry_bb = context.append_basic_block(func, "entry");
        builder.position_at_end(entry_bb);

        let s1 = func.get_nth_param(0).unwrap().into_pointer_value();
        let s2 = func.get_nth_param(1).unwrap().into_pointer_value();

        // Null checks for s1 and s2
        let s1_int = builder.build_ptr_to_int(s1, i64_type, "s1_int").unwrap();
        let s1_is_null = builder
            .build_int_compare(
                IntPredicate::EQ,
                s1_int,
                i64_type.const_int(0, false),
                "s1_null",
            )
            .unwrap();

        let s2_int = builder.build_ptr_to_int(s2, i64_type, "s2_int").unwrap();
        let s2_is_null = builder
            .build_int_compare(
                IntPredicate::EQ,
                s2_int,
                i64_type.const_int(0, false),
                "s2_null",
            )
            .unwrap();

        let dummy_empty = builder
            .build_global_string_ptr("", "empty_str")
            .unwrap()
            .as_basic_value_enum()
            .into_pointer_value();

        // Safe strlen calls
        let safe_s1_len = builder
            .build_select(s1_is_null, dummy_empty, s1, "safe_s1_len")
            .unwrap()
            .into_pointer_value();
        let len1_call = builder
            .build_call(strlen_fn, &[safe_s1_len.into()], "len1")
            .unwrap();
        let len1 = len1_call
            .try_as_basic_value()
            .basic()
            .unwrap()
            .into_int_value();

        let safe_s2_len = builder
            .build_select(s2_is_null, dummy_empty, s2, "safe_s2_len")
            .unwrap()
            .into_pointer_value();
        let len2_call = builder
            .build_call(strlen_fn, &[safe_s2_len.into()], "len2")
            .unwrap();
        let len2 = len2_call
            .try_as_basic_value()
            .basic()
            .unwrap()
            .into_int_value();

        let total_chars = builder.build_int_add(len1, len2, "total_chars").unwrap();
        let total_len = builder
            .build_int_add(total_chars, i64_type.const_int(1, false), "total_len")
            .unwrap();

        let mem_call = builder
            .build_call(malloc_fn, &[total_len.into()], "buf")
            .unwrap();
        let buf = mem_call
            .try_as_basic_value()
            .basic()
            .unwrap()
            .into_pointer_value();

        // Copy s1
        let safe_s1 = builder
            .build_select(s1_is_null, buf, s1, "safe_s1")
            .unwrap()
            .into_pointer_value();
        let _ = builder.build_call(memcpy_fn, &[buf.into(), safe_s1.into(), len1.into()], "");

        // Copy s2 at buf + len1
        let buf_int = builder.build_ptr_to_int(buf, i64_type, "buf_int").unwrap();
        let dest2_int = builder.build_int_add(buf_int, len1, "dest2_int").unwrap();
        let dest2 = builder
            .build_int_to_ptr(dest2_int, i8_ptr, "dest2")
            .unwrap();
        let safe_s2 = builder
            .build_select(s2_is_null, buf, s2, "safe_s2")
            .unwrap()
            .into_pointer_value();
        let _ = builder.build_call(memcpy_fn, &[dest2.into(), safe_s2.into(), len2.into()], "");

        // Null terminator at buf + total_chars
        let null_pos_int = builder
            .build_int_add(buf_int, total_chars, "null_pos_int")
            .unwrap();
        let null_pos = builder
            .build_int_to_ptr(null_pos_int, i8_ptr, "null_pos")
            .unwrap();
        let _ = builder.build_store(null_pos, context.i8_type().const_int(0, false));

        let _ = builder.build_return(Some(&buf));
        func
    }
}
