use bumpalo::collections::Vec;
use inkwell::context::Context;
use inkwell::types::BasicTypeEnum::{self, *};
use inkwell::types::{ArrayType, BasicType, FunctionType, IntType, PointerType, StructType};
use inkwell::values::BasicValueEnum;
use inkwell::AddressSpace;
use roc_mono::layout::{Builtin, Layout, UnionLayout};

/// TODO could this be added to Inkwell itself as a method on BasicValueEnum?
pub fn get_ptr_type<'ctx>(
    bt_enum: &BasicTypeEnum<'ctx>,
    address_space: AddressSpace,
) -> PointerType<'ctx> {
    match bt_enum {
        ArrayType(typ) => typ.ptr_type(address_space),
        IntType(typ) => typ.ptr_type(address_space),
        FloatType(typ) => typ.ptr_type(address_space),
        PointerType(typ) => typ.ptr_type(address_space),
        StructType(typ) => typ.ptr_type(address_space),
        VectorType(typ) => typ.ptr_type(address_space),
    }
}

/// TODO could this be added to Inkwell itself as a method on BasicValueEnum?
pub fn get_fn_type<'ctx>(
    bt_enum: &BasicTypeEnum<'ctx>,
    arg_types: &[BasicTypeEnum<'ctx>],
) -> FunctionType<'ctx> {
    match bt_enum {
        ArrayType(typ) => typ.fn_type(arg_types, false),
        IntType(typ) => typ.fn_type(arg_types, false),
        FloatType(typ) => typ.fn_type(arg_types, false),
        PointerType(typ) => typ.fn_type(arg_types, false),
        StructType(typ) => typ.fn_type(arg_types, false),
        VectorType(typ) => typ.fn_type(arg_types, false),
    }
}

/// TODO could this be added to Inkwell itself as a method on BasicValueEnum?
pub fn get_array_type<'ctx>(bt_enum: &BasicTypeEnum<'ctx>, size: u32) -> ArrayType<'ctx> {
    match bt_enum {
        ArrayType(typ) => typ.array_type(size),
        IntType(typ) => typ.array_type(size),
        FloatType(typ) => typ.array_type(size),
        PointerType(typ) => typ.array_type(size),
        StructType(typ) => typ.array_type(size),
        VectorType(typ) => typ.array_type(size),
    }
}

/// TODO could this be added to Inkwell itself as a method on BasicValueEnum?
pub fn as_const_zero<'ctx>(bt_enum: &BasicTypeEnum<'ctx>) -> BasicValueEnum<'ctx> {
    match bt_enum {
        ArrayType(typ) => typ.const_zero().into(),
        IntType(typ) => typ.const_zero().into(),
        FloatType(typ) => typ.const_zero().into(),
        PointerType(typ) => typ.const_zero().into(),
        StructType(typ) => typ.const_zero().into(),
        VectorType(typ) => typ.const_zero().into(),
    }
}

fn basic_type_from_function_layout<'a, 'ctx, 'env>(
    env: &crate::llvm::build::Env<'a, 'ctx, 'env>,
    args: &[Layout<'_>],
    closure_type: Option<BasicTypeEnum<'ctx>>,
    ret_layout: &Layout<'_>,
) -> BasicTypeEnum<'ctx> {
    let ret_type = basic_type_from_layout(env, &ret_layout);
    let mut arg_basic_types = Vec::with_capacity_in(args.len(), env.arena);

    for arg_layout in args.iter() {
        arg_basic_types.push(basic_type_from_layout(env, arg_layout));
    }

    if let Some(closure) = closure_type {
        arg_basic_types.push(closure);
    }

    let fn_type = get_fn_type(&ret_type, arg_basic_types.into_bump_slice());
    let ptr_type = fn_type.ptr_type(AddressSpace::Generic);

    ptr_type.as_basic_type_enum()
}

fn basic_type_from_record<'a, 'ctx, 'env>(
    env: &crate::llvm::build::Env<'a, 'ctx, 'env>,
    fields: &[Layout<'_>],
) -> BasicTypeEnum<'ctx> {
    let mut field_types = Vec::with_capacity_in(fields.len(), env.arena);

    for field_layout in fields.iter() {
        field_types.push(basic_type_from_layout(env, field_layout));
    }

    env.context
        .struct_type(field_types.into_bump_slice(), false)
        .as_basic_type_enum()
}

pub fn basic_type_from_layout<'a, 'ctx, 'env>(
    env: &crate::llvm::build::Env<'a, 'ctx, 'env>,
    layout: &Layout<'_>,
) -> BasicTypeEnum<'ctx> {
    use Layout::*;

    match layout {
        FunctionPointer(args, ret_layout) => {
            basic_type_from_function_layout(env, args, None, ret_layout)
        }
        Closure(args, closure_layout, ret_layout) => {
            let closure_data_layout = closure_layout.as_block_of_memory_layout();
            let closure_data = basic_type_from_layout(env, &closure_data_layout);

            let function_pointer =
                basic_type_from_function_layout(env, args, Some(closure_data), ret_layout);

            env.context
                .struct_type(&[function_pointer, closure_data], false)
                .as_basic_type_enum()
        }
        Pointer(layout) => basic_type_from_layout(env, &layout)
            .ptr_type(AddressSpace::Generic)
            .into(),
        PhantomEmptyStruct => env.context.struct_type(&[], false).into(),
        Struct(sorted_fields) => basic_type_from_record(env, sorted_fields),
        Union(variant) => {
            use UnionLayout::*;
            match variant {
                Recursive(tags)
                | NullableWrapped {
                    other_tags: tags, ..
                } => {
                    let block = block_of_memory_slices(env.context, tags, env.ptr_bytes);
                    block.ptr_type(AddressSpace::Generic).into()
                }
                NullableUnwrapped { other_fields, .. } => {
                    let block =
                        block_of_memory_slices(env.context, &[&other_fields[1..]], env.ptr_bytes);
                    block.ptr_type(AddressSpace::Generic).into()
                }
                NonNullableUnwrapped(fields) => {
                    let block = block_of_memory_slices(env.context, &[fields], env.ptr_bytes);
                    block.ptr_type(AddressSpace::Generic).into()
                }
                NonRecursive(_) => block_of_memory(env.context, layout, env.ptr_bytes),
            }
        }
        RecursivePointer => {
            // TODO make this dynamic
            env.context
                .i64_type()
                .ptr_type(AddressSpace::Generic)
                .as_basic_type_enum()
        }

        Builtin(builtin) => basic_type_from_builtin(env, builtin),
    }
}

pub fn basic_type_from_builtin<'a, 'ctx, 'env>(
    env: &crate::llvm::build::Env<'a, 'ctx, 'env>,
    builtin: &Builtin<'_>,
) -> BasicTypeEnum<'ctx> {
    use Builtin::*;

    let context = env.context;
    let ptr_bytes = env.ptr_bytes;

    match builtin {
        Int128 => context.i128_type().as_basic_type_enum(),
        Int64 => context.i64_type().as_basic_type_enum(),
        Int32 => context.i32_type().as_basic_type_enum(),
        Int16 => context.i16_type().as_basic_type_enum(),
        Int8 => context.i8_type().as_basic_type_enum(),
        Int1 => context.bool_type().as_basic_type_enum(),
        Usize => ptr_int(context, ptr_bytes).as_basic_type_enum(),
        Float128 => context.f128_type().as_basic_type_enum(),
        Float64 => context.f64_type().as_basic_type_enum(),
        Float32 => context.f32_type().as_basic_type_enum(),
        Float16 => context.f16_type().as_basic_type_enum(),
        Dict(_, _) | EmptyDict => zig_dict_type(env).into(),
        Set(_) | EmptySet => zig_dict_type(env).into(),
        List(_, _) | EmptyList => zig_list_type(env).into(),
        Str | EmptyStr => zig_str_type(env).into(),
    }
}

pub fn block_of_memory_slices<'ctx>(
    context: &'ctx Context,
    layouts: &[&[Layout<'_>]],
    ptr_bytes: u32,
) -> BasicTypeEnum<'ctx> {
    let mut union_size = 0;
    for tag in layouts {
        let mut total = 0;
        for layout in tag.iter() {
            total += layout.stack_size(ptr_bytes as u32);
        }

        union_size = union_size.max(total);
    }

    block_of_memory_help(context, union_size)
}

pub fn block_of_memory<'ctx>(
    context: &'ctx Context,
    layout: &Layout<'_>,
    ptr_bytes: u32,
) -> BasicTypeEnum<'ctx> {
    // TODO make this dynamic
    let union_size = layout.stack_size(ptr_bytes as u32);

    block_of_memory_help(context, union_size)
}

fn block_of_memory_help(context: &Context, union_size: u32) -> BasicTypeEnum<'_> {
    // The memory layout of Union is a bit tricky.
    // We have tags with different memory layouts, that are part of the same type.
    // For llvm, all tags must have the same memory layout.
    //
    // So, we convert all tags to a layout of bytes of some size.
    // It turns out that encoding to i64 for as many elements as possible is
    // a nice optimization, the remainder is encoded as bytes.

    let num_i64 = union_size / 8;
    let num_i8 = union_size % 8;

    let i64_array_type = context.i64_type().array_type(num_i64).as_basic_type_enum();

    if num_i8 == 0 {
        // the object fits perfectly in some number of i64's
        // (i.e. the size is a multiple of 8 bytes)
        context.struct_type(&[i64_array_type], false).into()
    } else {
        // there are some trailing bytes at the end
        let i8_array_type = context.i8_type().array_type(num_i8).as_basic_type_enum();

        context
            .struct_type(&[i64_array_type, i8_array_type], false)
            .into()
    }
}

pub fn ptr_int(ctx: &Context, ptr_bytes: u32) -> IntType<'_> {
    match ptr_bytes {
        1 => ctx.i8_type(),
        2 => ctx.i16_type(),
        4 => ctx.i32_type(),
        8 => ctx.i64_type(),
        _ => panic!(
            "Invalid target: Roc does't support compiling to {}-bit systems.",
            ptr_bytes * 8
        ),
    }
}

pub fn zig_dict_type<'a, 'ctx, 'env>(
    env: &crate::llvm::build::Env<'a, 'ctx, 'env>,
) -> StructType<'ctx> {
    env.module.get_struct_type("dict.RocDict").unwrap()
}

pub fn zig_list_type<'a, 'ctx, 'env>(
    env: &crate::llvm::build::Env<'a, 'ctx, 'env>,
) -> StructType<'ctx> {
    env.module.get_struct_type("list.RocList").unwrap()
}

pub fn zig_str_type<'a, 'ctx, 'env>(
    env: &crate::llvm::build::Env<'a, 'ctx, 'env>,
) -> StructType<'ctx> {
    env.module.get_struct_type("str.RocStr").unwrap()
}
