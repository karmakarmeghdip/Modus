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
    pub str_split_fn: FunctionValue<'ctx>,
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

        // 16. extern char* strstr(const char* haystack, const char* needle);
        let strstr_fn = module.get_function("strstr").unwrap_or_else(|| {
            let fn_type = i8_ptr.fn_type(&[i8_ptr.into(), i8_ptr.into()], false);
            module.add_function("strstr", fn_type, None)
        });

        // 17. extern char* strdup(const char* s);
        let strdup_fn = module.get_function("strdup").unwrap_or_else(|| {
            let fn_type = i8_ptr.fn_type(&[i8_ptr.into()], false);
            module.add_function("strdup", fn_type, None)
        });

        // 18. extern char* strndup(const char* s, size_t n);
        let strndup_fn = module.get_function("strndup").unwrap_or_else(|| {
            let fn_type = i8_ptr.fn_type(&[i8_ptr.into(), i64_type.into()], false);
            module.add_function("strndup", fn_type, None)
        });

        // 19. Build helper: modus_str_split(s: ptr, delim: ptr) -> ptr
        let str_split_fn = module.get_function("modus_str_split").unwrap_or_else(|| {
            Self::build_str_split_fn(
                context, module, alloc_fn, strlen_fn, strstr_fn, strdup_fn, strndup_fn,
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
            str_split_fn,
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

    /// Emits `modus_str_split(s: ptr, delim: ptr) -> ptr`:
    /// Splits `s` by `delim`, constructs and returns a Modus Array of Strings:
    /// `{ i64 rc = 1, i64 len, i64 cap, ptr reserved, [ptr s0, ptr s1, ...] }`.
    fn build_str_split_fn(
        context: &'ctx Context,
        module: &Module<'ctx>,
        alloc_fn: FunctionValue<'ctx>,
        strlen_fn: FunctionValue<'ctx>,
        strstr_fn: FunctionValue<'ctx>,
        strdup_fn: FunctionValue<'ctx>,
        strndup_fn: FunctionValue<'ctx>,
    ) -> FunctionValue<'ctx> {
        let i8_ptr = context.ptr_type(AddressSpace::default());
        let i64_type = context.i64_type();
        let i8_type = context.i8_type();

        let fn_type = i8_ptr.fn_type(&[i8_ptr.into(), i8_ptr.into()], false);
        let func = module.add_function("modus_str_split", fn_type, None);

        let builder = context.create_builder();
        let entry_bb = context.append_basic_block(func, "entry");
        let empty_arr_bb = context.append_basic_block(func, "empty_arr");
        let single_arr_bb = context.append_basic_block(func, "single_arr");
        let char_split_bb = context.append_basic_block(func, "char_split");
        let char_loop_bb = context.append_basic_block(func, "char_loop");
        let char_body_bb = context.append_basic_block(func, "char_body");
        let char_done_bb = context.append_basic_block(func, "char_done");

        let delim_split_bb = context.append_basic_block(func, "delim_split");
        let count_loop_bb = context.append_basic_block(func, "count_loop");
        let count_body_bb = context.append_basic_block(func, "count_body");
        let alloc_arr_bb = context.append_basic_block(func, "alloc_arr");
        let fill_loop_bb = context.append_basic_block(func, "fill_loop");
        let fill_mid_bb = context.append_basic_block(func, "fill_mid");
        let fill_end_bb = context.append_basic_block(func, "fill_end");
        let ret_arr_bb = context.append_basic_block(func, "ret_arr");

        builder.position_at_end(entry_bb);
        let s_arg = func.get_nth_param(0).unwrap().into_pointer_value();
        let delim_arg = func.get_nth_param(1).unwrap().into_pointer_value();

        let s_int = builder.build_ptr_to_int(s_arg, i64_type, "s_int").unwrap();
        let is_s_null = builder
            .build_int_compare(
                IntPredicate::EQ,
                s_int,
                i64_type.const_int(0, false),
                "is_s_null",
            )
            .unwrap();
        let _ = builder.build_conditional_branch(is_s_null, empty_arr_bb, delim_split_bb);

        // empty_arr block
        builder.position_at_end(empty_arr_bb);
        let empty_alloc = builder
            .build_call(
                alloc_fn,
                &[i64_type.const_int(32, false).into()],
                "empty_buf",
            )
            .unwrap();
        let empty_buf = empty_alloc
            .try_as_basic_value()
            .basic()
            .unwrap()
            .into_pointer_value();
        unsafe {
            let p1 = builder
                .build_gep(i64_type, empty_buf, &[i64_type.const_int(1, false)], "p1")
                .unwrap();
            let _ = builder.build_store(p1, i64_type.const_int(0, false));
            let p2 = builder
                .build_gep(i64_type, empty_buf, &[i64_type.const_int(2, false)], "p2")
                .unwrap();
            let _ = builder.build_store(p2, i64_type.const_int(0, false));
            let p3 = builder
                .build_gep(i64_type, empty_buf, &[i64_type.const_int(3, false)], "p3")
                .unwrap();
            let _ = builder.build_store(p3, i64_type.const_int(0, false));
        }
        let _ = builder.build_return(Some(&empty_buf));

        // delim_split: compute s_len, delim_len
        builder.position_at_end(delim_split_bb);
        let s_len_call = builder
            .build_call(strlen_fn, &[s_arg.into()], "s_len")
            .unwrap();
        let s_len = s_len_call
            .try_as_basic_value()
            .basic()
            .unwrap()
            .into_int_value();

        let delim_int = builder
            .build_ptr_to_int(delim_arg, i64_type, "delim_int")
            .unwrap();
        let is_delim_null = builder
            .build_int_compare(
                IntPredicate::EQ,
                delim_int,
                i64_type.const_int(0, false),
                "is_delim_null",
            )
            .unwrap();

        let dummy_empty = builder
            .build_global_string_ptr("", "empty_delim")
            .unwrap()
            .as_basic_value_enum()
            .into_pointer_value();
        let safe_delim = builder
            .build_select(is_delim_null, dummy_empty, delim_arg, "safe_delim")
            .unwrap()
            .into_pointer_value();
        let delim_len_call = builder
            .build_call(strlen_fn, &[safe_delim.into()], "delim_len")
            .unwrap();
        let delim_len = delim_len_call
            .try_as_basic_value()
            .basic()
            .unwrap()
            .into_int_value();

        let s_is_empty = builder
            .build_int_compare(
                IntPredicate::EQ,
                s_len,
                i64_type.const_int(0, false),
                "s_is_empty",
            )
            .unwrap();
        let delim_is_empty = builder
            .build_int_compare(
                IntPredicate::EQ,
                delim_len,
                i64_type.const_int(0, false),
                "delim_is_empty",
            )
            .unwrap();

        let s_and_delim_empty = builder
            .build_and(s_is_empty, delim_is_empty, "s_and_delim_empty")
            .unwrap();
        let check_s_empty_bb = context.append_basic_block(func, "check_s_empty");
        let check_delim_empty_bb = context.append_basic_block(func, "check_delim_empty");

        let _ = builder.build_conditional_branch(s_and_delim_empty, empty_arr_bb, check_s_empty_bb);

        builder.position_at_end(check_s_empty_bb);
        let _ = builder.build_conditional_branch(s_is_empty, single_arr_bb, check_delim_empty_bb);

        // single_arr: returns array of length 1 containing strdup(s)
        builder.position_at_end(single_arr_bb);
        let single_alloc = builder
            .build_call(
                alloc_fn,
                &[i64_type.const_int(40, false).into()],
                "single_buf",
            )
            .unwrap();
        let single_buf = single_alloc
            .try_as_basic_value()
            .basic()
            .unwrap()
            .into_pointer_value();
        let dup_s_call = builder
            .build_call(strdup_fn, &[s_arg.into()], "dup_s")
            .unwrap();
        let dup_s = dup_s_call
            .try_as_basic_value()
            .basic()
            .unwrap()
            .into_pointer_value();
        unsafe {
            let p1 = builder
                .build_gep(i64_type, single_buf, &[i64_type.const_int(1, false)], "p1")
                .unwrap();
            let _ = builder.build_store(p1, i64_type.const_int(1, false));
            let p2 = builder
                .build_gep(i64_type, single_buf, &[i64_type.const_int(2, false)], "p2")
                .unwrap();
            let _ = builder.build_store(p2, i64_type.const_int(1, false));
            let p3 = builder
                .build_gep(i64_type, single_buf, &[i64_type.const_int(3, false)], "p3")
                .unwrap();
            let _ = builder.build_store(p3, i64_type.const_int(0, false));
            let p4 = builder
                .build_gep(i8_ptr, single_buf, &[i64_type.const_int(4, false)], "p4")
                .unwrap();
            let _ = builder.build_store(p4, dup_s);
        }
        let _ = builder.build_return(Some(&single_buf));

        // check_delim_empty: if delim_len == 0 -> char_split
        builder.position_at_end(check_delim_empty_bb);
        let _ = builder.build_conditional_branch(delim_is_empty, char_split_bb, count_loop_bb);

        // char_split: allocate (4 + s_len) * 8
        builder.position_at_end(char_split_bb);
        let char_arr_size = builder
            .build_int_mul(
                builder
                    .build_int_add(s_len, i64_type.const_int(4, false), "c_off")
                    .unwrap(),
                i64_type.const_int(8, false),
                "char_arr_size",
            )
            .unwrap();
        let char_alloc = builder
            .build_call(alloc_fn, &[char_arr_size.into()], "char_arr")
            .unwrap();
        let char_arr = char_alloc
            .try_as_basic_value()
            .basic()
            .unwrap()
            .into_pointer_value();
        unsafe {
            let p1 = builder
                .build_gep(i64_type, char_arr, &[i64_type.const_int(1, false)], "p1")
                .unwrap();
            let _ = builder.build_store(p1, s_len);
            let p2 = builder
                .build_gep(i64_type, char_arr, &[i64_type.const_int(2, false)], "p2")
                .unwrap();
            let _ = builder.build_store(p2, s_len);
            let p3 = builder
                .build_gep(i64_type, char_arr, &[i64_type.const_int(3, false)], "p3")
                .unwrap();
            let _ = builder.build_store(p3, i64_type.const_int(0, false));
        }
        let char_idx_alloca = builder.build_alloca(i64_type, "char_idx").unwrap();
        let _ = builder.build_store(char_idx_alloca, i64_type.const_int(0, false));
        let _ = builder.build_unconditional_branch(char_loop_bb);

        builder.position_at_end(char_loop_bb);
        let c_idx = builder
            .build_load(i64_type, char_idx_alloca, "c_idx")
            .unwrap()
            .into_int_value();
        let c_more = builder
            .build_int_compare(IntPredicate::SLT, c_idx, s_len, "c_more")
            .unwrap();
        let _ = builder.build_conditional_branch(c_more, char_body_bb, char_done_bb);

        builder.position_at_end(char_body_bb);
        let char_src_ptr = unsafe {
            builder
                .build_gep(i8_type, s_arg, &[c_idx], "char_src_ptr")
                .unwrap()
        };
        let one_char_call = builder
            .build_call(
                strndup_fn,
                &[char_src_ptr.into(), i64_type.const_int(1, false).into()],
                "one_ch",
            )
            .unwrap();
        let one_ch = one_char_call
            .try_as_basic_value()
            .basic()
            .unwrap()
            .into_pointer_value();
        let slot_idx = builder
            .build_int_add(c_idx, i64_type.const_int(4, false), "slot_idx")
            .unwrap();
        let slot_ptr = unsafe {
            builder
                .build_gep(i8_ptr, char_arr, &[slot_idx], "slot_ptr")
                .unwrap()
        };
        let _ = builder.build_store(slot_ptr, one_ch);

        let next_c_idx = builder
            .build_int_add(c_idx, i64_type.const_int(1, false), "next_c_idx")
            .unwrap();
        let _ = builder.build_store(char_idx_alloca, next_c_idx);
        let _ = builder.build_unconditional_branch(char_loop_bb);

        builder.position_at_end(char_done_bb);
        let _ = builder.build_return(Some(&char_arr));

        // Delimiter split (delim_len > 0):
        // Pass 1: Count
        builder.position_at_end(count_loop_bb);
        let count_alloca = builder.build_alloca(i64_type, "count").unwrap();
        let _ = builder.build_store(count_alloca, i64_type.const_int(1, false));
        let curr_p_alloca = builder.build_alloca(i8_ptr, "curr_p").unwrap();
        let _ = builder.build_store(curr_p_alloca, s_arg);

        let count_cond_bb = context.append_basic_block(func, "count_cond");
        let _ = builder.build_unconditional_branch(count_cond_bb);

        builder.position_at_end(count_cond_bb);
        let curr_p = builder
            .build_load(i8_ptr, curr_p_alloca, "curr_p")
            .unwrap()
            .into_pointer_value();
        let find_call = builder
            .build_call(strstr_fn, &[curr_p.into(), safe_delim.into()], "found")
            .unwrap();
        let found = find_call
            .try_as_basic_value()
            .basic()
            .unwrap()
            .into_pointer_value();
        let found_int = builder
            .build_ptr_to_int(found, i64_type, "found_int")
            .unwrap();
        let found_is_null = builder
            .build_int_compare(
                IntPredicate::EQ,
                found_int,
                i64_type.const_int(0, false),
                "f_null",
            )
            .unwrap();
        let _ = builder.build_conditional_branch(found_is_null, alloc_arr_bb, count_body_bb);

        builder.position_at_end(count_body_bb);
        let old_count = builder
            .build_load(i64_type, count_alloca, "old_c")
            .unwrap()
            .into_int_value();
        let new_count = builder
            .build_int_add(old_count, i64_type.const_int(1, false), "new_c")
            .unwrap();
        let _ = builder.build_store(count_alloca, new_count);

        let next_p = unsafe {
            builder
                .build_gep(i8_type, found, &[delim_len], "next_p")
                .unwrap()
        };
        let _ = builder.build_store(curr_p_alloca, next_p);
        let _ = builder.build_unconditional_branch(count_cond_bb);

        // Pass 2: Allocate array
        builder.position_at_end(alloc_arr_bb);
        let total_parts = builder
            .build_load(i64_type, count_alloca, "total_parts")
            .unwrap()
            .into_int_value();
        let arr_size_bytes = builder
            .build_int_mul(
                builder
                    .build_int_add(total_parts, i64_type.const_int(4, false), "arr_slots")
                    .unwrap(),
                i64_type.const_int(8, false),
                "arr_size_bytes",
            )
            .unwrap();
        let main_alloc = builder
            .build_call(alloc_fn, &[arr_size_bytes.into()], "main_arr")
            .unwrap();
        let main_arr = main_alloc
            .try_as_basic_value()
            .basic()
            .unwrap()
            .into_pointer_value();
        unsafe {
            let p1 = builder
                .build_gep(i64_type, main_arr, &[i64_type.const_int(1, false)], "p1")
                .unwrap();
            let _ = builder.build_store(p1, total_parts);
            let p2 = builder
                .build_gep(i64_type, main_arr, &[i64_type.const_int(2, false)], "p2")
                .unwrap();
            let _ = builder.build_store(p2, total_parts);
            let p3 = builder
                .build_gep(i64_type, main_arr, &[i64_type.const_int(3, false)], "p3")
                .unwrap();
            let _ = builder.build_store(p3, i64_type.const_int(0, false));
        }

        let fill_idx_alloca = builder.build_alloca(i64_type, "fill_idx").unwrap();
        let _ = builder.build_store(fill_idx_alloca, i64_type.const_int(0, false));
        let fill_p_alloca = builder.build_alloca(i8_ptr, "fill_p").unwrap();
        let _ = builder.build_store(fill_p_alloca, s_arg);

        let _ = builder.build_unconditional_branch(fill_loop_bb);

        builder.position_at_end(fill_loop_bb);
        let fill_p = builder
            .build_load(i8_ptr, fill_p_alloca, "fill_p")
            .unwrap()
            .into_pointer_value();
        let fill_find_call = builder
            .build_call(strstr_fn, &[fill_p.into(), safe_delim.into()], "f_match")
            .unwrap();
        let f_match = fill_find_call
            .try_as_basic_value()
            .basic()
            .unwrap()
            .into_pointer_value();
        let f_match_int = builder
            .build_ptr_to_int(f_match, i64_type, "f_int")
            .unwrap();
        let f_match_null = builder
            .build_int_compare(
                IntPredicate::EQ,
                f_match_int,
                i64_type.const_int(0, false),
                "f_match_null",
            )
            .unwrap();
        let _ = builder.build_conditional_branch(f_match_null, fill_end_bb, fill_mid_bb);

        builder.position_at_end(fill_mid_bb);
        let fill_p_int = builder
            .build_ptr_to_int(fill_p, i64_type, "fill_p_int")
            .unwrap();
        let seg_len = builder
            .build_int_sub(f_match_int, fill_p_int, "seg_len")
            .unwrap();
        let seg_call = builder
            .build_call(strndup_fn, &[fill_p.into(), seg_len.into()], "seg_str")
            .unwrap();
        let seg_str = seg_call
            .try_as_basic_value()
            .basic()
            .unwrap()
            .into_pointer_value();

        let f_idx = builder
            .build_load(i64_type, fill_idx_alloca, "f_idx")
            .unwrap()
            .into_int_value();
        let slot = builder
            .build_int_add(f_idx, i64_type.const_int(4, false), "slot")
            .unwrap();
        let dest_slot = unsafe {
            builder
                .build_gep(i8_ptr, main_arr, &[slot], "dest_slot")
                .unwrap()
        };
        let _ = builder.build_store(dest_slot, seg_str);

        let next_f_idx = builder
            .build_int_add(f_idx, i64_type.const_int(1, false), "next_f_idx")
            .unwrap();
        let _ = builder.build_store(fill_idx_alloca, next_f_idx);

        let next_fill_p = unsafe {
            builder
                .build_gep(i8_type, f_match, &[delim_len], "next_fill_p")
                .unwrap()
        };
        let _ = builder.build_store(fill_p_alloca, next_fill_p);
        let _ = builder.build_unconditional_branch(fill_loop_bb);

        // fill_end: last segment strdup(fill_p)
        builder.position_at_end(fill_end_bb);
        let last_seg_call = builder
            .build_call(strdup_fn, &[fill_p.into()], "last_seg")
            .unwrap();
        let last_seg = last_seg_call
            .try_as_basic_value()
            .basic()
            .unwrap()
            .into_pointer_value();
        let last_idx = builder
            .build_load(i64_type, fill_idx_alloca, "last_idx")
            .unwrap()
            .into_int_value();
        let last_slot = builder
            .build_int_add(last_idx, i64_type.const_int(4, false), "last_slot")
            .unwrap();
        let dest_last_slot = unsafe {
            builder
                .build_gep(i8_ptr, main_arr, &[last_slot], "dest_last_slot")
                .unwrap()
        };
        let _ = builder.build_store(dest_last_slot, last_seg);
        let _ = builder.build_unconditional_branch(ret_arr_bb);

        builder.position_at_end(ret_arr_bb);
        let _ = builder.build_return(Some(&main_arr));

        func
    }
}
