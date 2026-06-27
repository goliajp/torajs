//! Pass 0 `declare_intrinsic` group: fs module substrate (v0.3 #1).
//!
//! chunk 118 continuation of the Pass 0 multi-sub-sibling split
//! (chunks 110-117). 8 declarations covering sync read / write /
//! exists / append / unlink / mkdir / readdir / size.
//!
//! All take a path `Str`; write/append also take a body `Str`.
//! Return shapes:
//! - `read_file_sync(p) -> Str`
//! - `write_file_sync(p, body) -> Void`
//! - `exists_sync(p) -> Bool`
//! - `append_file_sync(p, body) -> Void`
//! - `unlink_sync(p) -> Void`
//! - `mkdir_sync(p) -> Void`
//! - `readdir_sync(p) -> Ptr` (T-18.b: Array<string>, ABI-typed as
//!   Ptr at the intrinsic boundary; call site re-types via Member-
//!   call dispatch knowing the static return type)
//! - `size_sync(p) -> I64` (T-18.c: size in bytes, -1 on stat
//!   failure or non-regular file; consumed by `Bun.file(p).size` +
//!   future `fs.statSync(p).size`)

use std::collections::HashMap;

use crate::ssa::{FuncId, Module, Type};
use crate::ssa_lower::declare_intrinsic;

pub(crate) struct FsIds {
    pub fs_read_file_sync: FuncId,
    pub fs_write_file_sync: FuncId,
    pub fs_exists_sync: FuncId,
    pub fs_append_file_sync: FuncId,
    pub fs_unlink_sync: FuncId,
    pub fs_mkdir_sync: FuncId,
    pub fs_readdir_sync: FuncId,
    pub fs_size_sync: FuncId,
}

pub(crate) fn declare(module: &mut Module, fn_table: &mut HashMap<String, FuncId>) -> FsIds {
    FsIds {
        fs_read_file_sync: declare_intrinsic(
            module,
            fn_table,
            "__torajs_fs_read_file_sync",
            &[Type::Str],
            Type::Str,
        ),
        fs_write_file_sync: declare_intrinsic(
            module,
            fn_table,
            "__torajs_fs_write_file_sync",
            &[Type::Str, Type::Str],
            Type::Void,
        ),
        fs_exists_sync: declare_intrinsic(
            module,
            fn_table,
            "__torajs_fs_exists_sync",
            &[Type::Str],
            Type::Bool,
        ),
        fs_append_file_sync: declare_intrinsic(
            module,
            fn_table,
            "__torajs_fs_append_file_sync",
            &[Type::Str, Type::Str],
            Type::Void,
        ),
        fs_unlink_sync: declare_intrinsic(
            module,
            fn_table,
            "__torajs_fs_unlink_sync",
            &[Type::Str],
            Type::Void,
        ),
        fs_mkdir_sync: declare_intrinsic(
            module,
            fn_table,
            "__torajs_fs_mkdir_sync",
            &[Type::Str],
            Type::Void,
        ),
        fs_readdir_sync: declare_intrinsic(
            module,
            fn_table,
            "__torajs_fs_readdir_sync",
            &[Type::Str],
            Type::Ptr,
        ),
        fs_size_sync: declare_intrinsic(
            module,
            fn_table,
            "__torajs_fs_size_sync",
            &[Type::Str],
            Type::I64,
        ),
    }
}
