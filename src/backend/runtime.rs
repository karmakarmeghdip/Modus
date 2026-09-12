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
use inkwell::values::FunctionValue;

/// Manages runtime declarations and helper functions in an LLVM module.
pub struct Runtime<'ctx> {
    pub context: &'ctx Context,
    pub malloc_fn: FunctionValue<'ctx>,
    pub free_fn: FunctionValue<'ctx>,
    pub puts_fn: FunctionValue<'ctx>,
    pub printf_fn: FunctionValue<'ctx>,
    pub alloc_fn: FunctionValue<'ctx>,
    pub inc_ref_fn: FunctionValue<'ctx>,
    pub dec_ref_fn: FunctionValue<'ctx>,
    pub is_unique_fn: FunctionValue<'ctx>,
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

        // 5. Build helper: modus_alloc(size: i64) -> ptr
        let alloc_fn = module
            .get_function("modus_alloc")
            .unwrap_or_else(|| Self::build_alloc_fn(context, module, malloc_fn));

        // 6. Build helper: modus_inc_ref(ptr: ptr) -> void
        let inc_ref_fn = module
            .get_function("modus_inc_ref")
            .unwrap_or_else(|| Self::build_inc_ref_fn(context, module));

        // 7. Build helper: modus_dec_ref(ptr: ptr) -> void
        let dec_ref_fn = module
            .get_function("modus_dec_ref")
            .unwrap_or_else(|| Self::build_dec_ref_fn(context, module, free_fn));

        // 8. Build helper: modus_is_unique(ptr: ptr) -> bool
        let is_unique_fn = module
            .get_function("modus_is_unique")
            .unwrap_or_else(|| Self::build_is_unique_fn(context, module));

        Self {
            context,
            malloc_fn,
            free_fn,
            puts_fn,
            printf_fn,
            alloc_fn,
            inc_ref_fn,
            dec_ref_fn,
            is_unique_fn,
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
}
