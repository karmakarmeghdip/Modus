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
    pub strcmp_fn: FunctionValue<'ctx>,
    pub fs_read_dir_fn: FunctionValue<'ctx>,
    pub fs_rename_fn: FunctionValue<'ctx>,
    pub array_builder_new_fn: FunctionValue<'ctx>,
    pub array_builder_push_fn: FunctionValue<'ctx>,
    pub array_builder_build_fn: FunctionValue<'ctx>,
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

        // 13. extern int strcmp(const char* s1, const char* s2);
        let strcmp_fn = module.get_function("strcmp").unwrap_or_else(|| {
            let fn_type = i32_type.fn_type(&[i8_ptr.into(), i8_ptr.into()], false);
            module.add_function("strcmp", fn_type, None)
        });

        // 14. Build helper: modus_fs_read_dir(path: ptr) -> ptr
        let fs_read_dir_fn = module
            .get_function("modus_fs_read_dir")
            .unwrap_or_else(|| Self::build_fs_read_dir_fn(context, module, alloc_fn, strcmp_fn));

        // 15. Build helper: modus_fs_rename(old: ptr, new: ptr) -> i32
        let rename_libc_fn = module.get_function("rename").unwrap_or_else(|| {
            let fn_type = i32_type.fn_type(&[i8_ptr.into(), i8_ptr.into()], false);
            module.add_function("rename", fn_type, None)
        });
        let fs_rename_fn = module.get_function("modus_fs_rename").unwrap_or_else(|| {
            let fn_type = i32_type.fn_type(&[i8_ptr.into(), i8_ptr.into()], false);
            let func = module.add_function("modus_fs_rename", fn_type, None);
            let b = context.create_builder();
            let bb = context.append_basic_block(func, "entry");
            b.position_at_end(bb);
            let p1 = func.get_nth_param(0).unwrap();
            let p2 = func.get_nth_param(1).unwrap();
            let res = b
                .build_call(rename_libc_fn, &[p1.into(), p2.into()], "call_rename")
                .unwrap()
                .try_as_basic_value()
                .basic()
                .unwrap()
                .into_int_value();
            let _ = b.build_return(Some(&res));
            func
        });

        // 16. Build array builder helpers:
        let array_builder_new_fn = module
            .get_function("modus_array_builder_new")
            .unwrap_or_else(|| Self::build_array_builder_new_fn(context, module, malloc_fn));

        let array_builder_push_fn = module
            .get_function("modus_array_builder_push")
            .unwrap_or_else(|| {
                Self::build_array_builder_push_fn(
                    context, module, malloc_fn, free_fn, memcpy_fn, inc_ref_fn, dec_ref_fn,
                )
            });

        let array_builder_build_fn = module
            .get_function("modus_array_builder_build")
            .unwrap_or_else(|| {
                Self::build_array_builder_build_fn(
                    context, module, malloc_fn, memcpy_fn, inc_ref_fn, dec_ref_fn,
                )
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
            strcmp_fn,
            fs_read_dir_fn,
            fs_rename_fn,
            array_builder_new_fn,
            array_builder_push_fn,
            array_builder_build_fn,
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

    /// Emits `modus_fs_read_dir(path: ptr) -> ptr`:
    /// Reads directory entries excluding "." and "..", constructs and returns a Modus Array of Strings:
    /// `{ i64 rc = 1, i64 len, i64 cap, ptr reserved, [ptr s0, ptr s1, ...] }`.
    fn build_fs_read_dir_fn(
        context: &'ctx Context,
        module: &Module<'ctx>,
        alloc_fn: FunctionValue<'ctx>,
        strcmp_fn: FunctionValue<'ctx>,
    ) -> FunctionValue<'ctx> {
        let i8_ptr = context.ptr_type(AddressSpace::default());
        let i64_type = context.i64_type();
        let i32_type = context.i32_type();
        let i8_type = context.i8_type();

        let opendir_fn = module.get_function("opendir").unwrap_or_else(|| {
            let fn_type = i8_ptr.fn_type(&[i8_ptr.into()], false);
            module.add_function("opendir", fn_type, None)
        });

        let readdir_fn = module.get_function("readdir").unwrap_or_else(|| {
            let fn_type = i8_ptr.fn_type(&[i8_ptr.into()], false);
            module.add_function("readdir", fn_type, None)
        });

        let closedir_fn = module.get_function("closedir").unwrap_or_else(|| {
            let fn_type = i32_type.fn_type(&[i8_ptr.into()], false);
            module.add_function("closedir", fn_type, None)
        });

        let strdup_fn = module.get_function("strdup").unwrap_or_else(|| {
            let fn_type = i8_ptr.fn_type(&[i8_ptr.into()], false);
            module.add_function("strdup", fn_type, None)
        });

        let fn_type = i8_ptr.fn_type(&[i8_ptr.into()], false);
        let func = module.add_function("modus_fs_read_dir", fn_type, None);

        let builder = context.create_builder();

        let entry_bb = context.append_basic_block(func, "entry");
        let empty_ret_bb = context.append_basic_block(func, "empty_ret");
        let count_loop_bb = context.append_basic_block(func, "count_loop");
        let count_check_bb = context.append_basic_block(func, "count_check");
        let count_inc_bb = context.append_basic_block(func, "count_inc");
        let count_done_bb = context.append_basic_block(func, "count_done");
        let open_second_bb = context.append_basic_block(func, "open_second");
        let fill_loop_bb = context.append_basic_block(func, "fill_loop");
        let fill_check_bb = context.append_basic_block(func, "fill_check");
        let fill_store_bb = context.append_basic_block(func, "fill_store");
        let fill_done_bb = context.append_basic_block(func, "fill_done");
        let ret_arr_bb = context.append_basic_block(func, "ret_arr");

        // Entry block
        builder.position_at_end(entry_bb);
        let path_arg = func.get_first_param().unwrap().into_pointer_value();

        // Global constant strings for "." and ".."
        let dot_str = builder
            .build_global_string_ptr(".", "dot")
            .unwrap()
            .as_basic_value_enum();
        let dotdot_str = builder
            .build_global_string_ptr("..", "dotdot")
            .unwrap()
            .as_basic_value_enum();

        let count_alloca = builder.build_alloca(i64_type, "count").unwrap();
        let _ = builder.build_store(count_alloca, i64_type.const_int(0, false));
        let idx_alloca = builder.build_alloca(i64_type, "idx").unwrap();
        let _ = builder.build_store(idx_alloca, i64_type.const_int(0, false));
        let arr_alloca = builder.build_alloca(i8_ptr, "arr_alloca").unwrap();

        let dir1_call = builder
            .build_call(opendir_fn, &[path_arg.into()], "dir1")
            .unwrap();
        let dir1_ptr = dir1_call
            .try_as_basic_value()
            .basic()
            .unwrap()
            .into_pointer_value();
        let dir1_int = builder
            .build_ptr_to_int(dir1_ptr, i64_type, "dir1_int")
            .unwrap();
        let is_dir1_null = builder
            .build_int_compare(
                IntPredicate::EQ,
                dir1_int,
                i64_type.const_int(0, false),
                "is_dir1_null",
            )
            .unwrap();
        let _ = builder.build_conditional_branch(is_dir1_null, empty_ret_bb, count_loop_bb);

        // empty_ret block
        builder.position_at_end(empty_ret_bb);
        let empty_alloc = builder
            .build_call(
                alloc_fn,
                &[i64_type.const_int(32, false).into()],
                "empty_arr",
            )
            .unwrap();
        let empty_ptr = empty_alloc
            .try_as_basic_value()
            .basic()
            .unwrap()
            .into_pointer_value();
        unsafe {
            let p1 = builder
                .build_gep(i64_type, empty_ptr, &[i64_type.const_int(1, false)], "p1")
                .unwrap();
            let _ = builder.build_store(p1, i64_type.const_int(0, false));
            let p2 = builder
                .build_gep(i64_type, empty_ptr, &[i64_type.const_int(2, false)], "p2")
                .unwrap();
            let _ = builder.build_store(p2, i64_type.const_int(0, false));
            let p3 = builder
                .build_gep(i64_type, empty_ptr, &[i64_type.const_int(3, false)], "p3")
                .unwrap();
            let _ = builder.build_store(p3, i64_type.const_int(0, false));
        }
        let _ = builder.build_return(Some(&empty_ptr));

        // count_loop block
        builder.position_at_end(count_loop_bb);
        let de1_call = builder
            .build_call(readdir_fn, &[dir1_ptr.into()], "de1")
            .unwrap();
        let de1_ptr = de1_call
            .try_as_basic_value()
            .basic()
            .unwrap()
            .into_pointer_value();
        let de1_int = builder
            .build_ptr_to_int(de1_ptr, i64_type, "de1_int")
            .unwrap();
        let is_de1_null = builder
            .build_int_compare(
                IntPredicate::EQ,
                de1_int,
                i64_type.const_int(0, false),
                "is_de1_null",
            )
            .unwrap();
        let _ = builder.build_conditional_branch(is_de1_null, count_done_bb, count_check_bb);

        // count_check block
        builder.position_at_end(count_check_bb);
        let d_name1 = unsafe {
            builder
                .build_gep(
                    i8_type,
                    de1_ptr,
                    &[i64_type.const_int(19, false)],
                    "d_name1",
                )
                .unwrap()
        };
        let cmp_dot1 = builder
            .build_call(strcmp_fn, &[d_name1.into(), dot_str.into()], "cmp_dot1")
            .unwrap()
            .try_as_basic_value()
            .basic()
            .unwrap()
            .into_int_value();
        let is_dot1 = builder
            .build_int_compare(
                IntPredicate::EQ,
                cmp_dot1,
                i32_type.const_int(0, false),
                "is_dot1",
            )
            .unwrap();

        let cmp_dotdot1 = builder
            .build_call(
                strcmp_fn,
                &[d_name1.into(), dotdot_str.into()],
                "cmp_dotdot1",
            )
            .unwrap()
            .try_as_basic_value()
            .basic()
            .unwrap()
            .into_int_value();
        let is_dotdot1 = builder
            .build_int_compare(
                IntPredicate::EQ,
                cmp_dotdot1,
                i32_type.const_int(0, false),
                "is_dotdot1",
            )
            .unwrap();

        let is_skip1 = builder.build_or(is_dot1, is_dotdot1, "is_skip1").unwrap();
        let _ = builder.build_conditional_branch(is_skip1, count_loop_bb, count_inc_bb);

        // count_inc block
        builder.position_at_end(count_inc_bb);
        let c = builder
            .build_load(i64_type, count_alloca, "c")
            .unwrap()
            .into_int_value();
        let c_next = builder
            .build_int_add(c, i64_type.const_int(1, false), "c_next")
            .unwrap();
        let _ = builder.build_store(count_alloca, c_next);
        let _ = builder.build_unconditional_branch(count_loop_bb);

        // count_done block
        builder.position_at_end(count_done_bb);
        let _ = builder.build_call(closedir_fn, &[dir1_ptr.into()], "");
        let total_count = builder
            .build_load(i64_type, count_alloca, "total_count")
            .unwrap()
            .into_int_value();
        let four = i64_type.const_int(4, false);
        let total_elems = builder
            .build_int_add(total_count, four, "total_elems")
            .unwrap();
        let eight = i64_type.const_int(8, false);
        let alloc_bytes = builder
            .build_int_mul(total_elems, eight, "alloc_bytes")
            .unwrap();
        let arr_call = builder
            .build_call(alloc_fn, &[alloc_bytes.into()], "arr")
            .unwrap();
        let arr_ptr = arr_call
            .try_as_basic_value()
            .basic()
            .unwrap()
            .into_pointer_value();
        let _ = builder.build_store(arr_alloca, arr_ptr);

        unsafe {
            let p1 = builder
                .build_gep(i64_type, arr_ptr, &[i64_type.const_int(1, false)], "p1")
                .unwrap();
            let _ = builder.build_store(p1, total_count);
            let p2 = builder
                .build_gep(i64_type, arr_ptr, &[i64_type.const_int(2, false)], "p2")
                .unwrap();
            let _ = builder.build_store(p2, total_count);
            let p3 = builder
                .build_gep(i64_type, arr_ptr, &[i64_type.const_int(3, false)], "p3")
                .unwrap();
            let _ = builder.build_store(p3, i64_type.const_int(0, false));
        }

        let has_elements = builder
            .build_int_compare(
                IntPredicate::SGT,
                total_count,
                i64_type.const_int(0, false),
                "has_elements",
            )
            .unwrap();
        let _ = builder.build_conditional_branch(has_elements, open_second_bb, ret_arr_bb);

        // open_second block
        builder.position_at_end(open_second_bb);
        let dir2_call = builder
            .build_call(opendir_fn, &[path_arg.into()], "dir2")
            .unwrap();
        let dir2_ptr = dir2_call
            .try_as_basic_value()
            .basic()
            .unwrap()
            .into_pointer_value();
        let dir2_int = builder
            .build_ptr_to_int(dir2_ptr, i64_type, "dir2_int")
            .unwrap();
        let is_dir2_null = builder
            .build_int_compare(
                IntPredicate::EQ,
                dir2_int,
                i64_type.const_int(0, false),
                "is_dir2_null",
            )
            .unwrap();
        let _ = builder.build_conditional_branch(is_dir2_null, ret_arr_bb, fill_loop_bb);

        // fill_loop block
        builder.position_at_end(fill_loop_bb);
        let de2_call = builder
            .build_call(readdir_fn, &[dir2_ptr.into()], "de2")
            .unwrap();
        let de2_ptr = de2_call
            .try_as_basic_value()
            .basic()
            .unwrap()
            .into_pointer_value();
        let de2_int = builder
            .build_ptr_to_int(de2_ptr, i64_type, "de2_int")
            .unwrap();
        let is_de2_null = builder
            .build_int_compare(
                IntPredicate::EQ,
                de2_int,
                i64_type.const_int(0, false),
                "is_de2_null",
            )
            .unwrap();
        let _ = builder.build_conditional_branch(is_de2_null, fill_done_bb, fill_check_bb);

        // fill_check block
        builder.position_at_end(fill_check_bb);
        let d_name2 = unsafe {
            builder
                .build_gep(
                    i8_type,
                    de2_ptr,
                    &[i64_type.const_int(19, false)],
                    "d_name2",
                )
                .unwrap()
        };
        let cmp_dot2 = builder
            .build_call(strcmp_fn, &[d_name2.into(), dot_str.into()], "cmp_dot2")
            .unwrap()
            .try_as_basic_value()
            .basic()
            .unwrap()
            .into_int_value();
        let is_dot2 = builder
            .build_int_compare(
                IntPredicate::EQ,
                cmp_dot2,
                i32_type.const_int(0, false),
                "is_dot2",
            )
            .unwrap();

        let cmp_dotdot2 = builder
            .build_call(
                strcmp_fn,
                &[d_name2.into(), dotdot_str.into()],
                "cmp_dotdot2",
            )
            .unwrap()
            .try_as_basic_value()
            .basic()
            .unwrap()
            .into_int_value();
        let is_dotdot2 = builder
            .build_int_compare(
                IntPredicate::EQ,
                cmp_dotdot2,
                i32_type.const_int(0, false),
                "is_dotdot2",
            )
            .unwrap();

        let is_skip2 = builder.build_or(is_dot2, is_dotdot2, "is_skip2").unwrap();
        let _ = builder.build_conditional_branch(is_skip2, fill_loop_bb, fill_store_bb);

        // fill_store block
        builder.position_at_end(fill_store_bb);
        let dup_call = builder
            .build_call(strdup_fn, &[d_name2.into()], "dup_name")
            .unwrap();
        let dup_ptr = dup_call
            .try_as_basic_value()
            .basic()
            .unwrap()
            .into_pointer_value();
        let curr_idx = builder
            .build_load(i64_type, idx_alloca, "curr_idx")
            .unwrap()
            .into_int_value();
        let slot_offset = builder
            .build_int_add(curr_idx, i64_type.const_int(4, false), "slot_offset")
            .unwrap();

        let arr_val = builder
            .build_load(i8_ptr, arr_alloca, "arr_val")
            .unwrap()
            .into_pointer_value();
        let elem_slot = unsafe {
            builder
                .build_gep(i8_ptr, arr_val, &[slot_offset], "elem_slot")
                .unwrap()
        };
        let _ = builder.build_store(elem_slot, dup_ptr);

        let next_idx = builder
            .build_int_add(curr_idx, i64_type.const_int(1, false), "next_idx")
            .unwrap();
        let _ = builder.build_store(idx_alloca, next_idx);
        let _ = builder.build_unconditional_branch(fill_loop_bb);

        // fill_done block
        builder.position_at_end(fill_done_bb);
        let _ = builder.build_call(closedir_fn, &[dir2_ptr.into()], "");
        let _ = builder.build_unconditional_branch(ret_arr_bb);

        // ret_arr block
        builder.position_at_end(ret_arr_bb);
        let final_arr = builder
            .build_load(i8_ptr, arr_alloca, "final_arr")
            .unwrap()
            .into_pointer_value();
        let _ = builder.build_return(Some(&final_arr));

        func
    }
    /// Emits `modus_array_builder_new(cap: i64) -> ptr`:
    /// Allocates buffer of size (4 + max(cap, 4)) * 8 bytes, sets rc = 1, len = 0, cap = max(cap, 4), reserved = 0.
    fn build_array_builder_new_fn(
        context: &'ctx Context,
        module: &Module<'ctx>,
        malloc_fn: FunctionValue<'ctx>,
    ) -> FunctionValue<'ctx> {
        let i8_ptr = context.ptr_type(AddressSpace::default());
        let i64_type = context.i64_type();
        let fn_type = i8_ptr.fn_type(&[i64_type.into()], false);
        let func = module.add_function("modus_array_builder_new", fn_type, None);

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

    /// Emits `modus_array_builder_push(builder: ptr, elem: i64, elem_is_heap: i1) -> ptr`:
    /// Functional-But-In-Place (FBIP) accumulation.
    /// If rc == 1:
    ///   if len < cap: in-place write elem at 4 + len, len += 1, return builder
    ///   if len == cap: double cap, allocate new buffer, copy 4 header words + len elements, write elem, free(old), return new
    /// If rc > 1:
    ///   copy-on-write: allocate new buffer, copy elements (inc_ref if heap), write elem, dec_ref(old), return new
    fn build_array_builder_push_fn(
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
        let func = module.add_function("modus_array_builder_push", fn_type, None);

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

    /// Emits `modus_array_builder_build(builder: ptr, elem_is_heap: i1) -> ptr`:
    /// Finalizes builder into immutable array [T].
    /// If rc == 1: Zero-copy! Returns builder directly.
    /// If rc > 1: Clones exact-sized array [T], inc_refs elements if heap, dec_refs old builder, returns new array.
    fn build_array_builder_build_fn(
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
        let func = module.add_function("modus_array_builder_build", fn_type, None);

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
}
