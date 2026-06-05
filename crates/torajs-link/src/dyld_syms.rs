//! macOS libSystem-resolved dyld symbol whitelist for the #9 SD substrate.
//!
//! Production `tr` binaries (currently emitted by `ssa_inkwell` →
//! LLVM → host `ld`) link a single `LC_LOAD_DYLIB` against
//! `/usr/lib/libSystem.B.dylib` and let dyld resolve these symbols at
//! exec time. The #9 self-research swap path needs the same dyld
//! substrate; this module is its data layer.
//!
//! `is_libsystem_resolved(name)` classifies an `_extern` symbol seen
//! during `compute_required_members` against the static set below.
//! Matches are recorded as dyld imports (LC_LOAD_DYLIB + per-sym
//! trampoline in SD-2) instead of being flagged as
//! `ArchiveLinkError::UnresolvedExterns`.
//!
//! ## Sourcing
//!
//! The list was harvested by `examples/probe_real_link.rs`:
//!
//! ```text
//! $ cargo run --release -p torajs-link --example probe_real_link \
//!     -- ___torajs_str_alloc
//! layout ERR: Link(UnresolvedExterns { names: [...192 entries...] })
//! ```
//!
//! against the full 32-archive production set at HEAD `cd546fa`, then
//! cross-referenced with `otool -L /tmp/_smoke` of an `ssa_inkwell`-built
//! `console.log(1+1)` binary which shows a single LC_LOAD_DYLIB
//! `/usr/lib/libSystem.B.dylib (current 1356.0.0)`. Every entry below
//! is a documented export of libSystem.B on macOS 14+.
//!
//! ## Excluded by design
//!
//! - **`_curl_easy_{cleanup,getinfo,init,perform,setopt}`** —
//!   libcurl.4.dylib, not libSystem. Needs a separate `LC_LOAD_DYLIB`;
//!   SD-1 keeps scope tight to libSystem and lets these surface as
//!   residual `UnresolvedExterns` so SD-1+ knows what to add next.
//!
//! - **`_print_bool` / `_print_f64` / `_print_i64`** — toolchain
//!   helper functions currently emitted directly into the final
//!   binary by `ssa_inkwell::compile`'s LLVM IR pipeline (see
//!   `crates/torajs-core/src/ssa_inkwell.rs::define_print_*`). SD-4
//!   (atomic swap to torajs-codegen) must re-emit them as part of the
//!   replacement pipeline (or move them to a staticlib); they are not
//!   dyld symbols.

use std::collections::BTreeSet;
use std::sync::OnceLock;

/// Per-import `lib_ordinal` value emitted into the
/// `dyld_chained_import` entries and used by the linker to pick
/// which `LC_LOAD_DYLIB` resolves the binding. `1` = first
/// `LC_LOAD_DYLIB` in the binary (libSystem), `2` = second
/// (libcurl). Matches Apple `ld64`'s `BindOrdinal` numbering.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum DyldLib {
    LibSystem = 1,
    LibCurl = 2,
}

impl DyldLib {
    /// Lib ordinal as the `u8` the chained-fixups encoder packs
    /// into `dyld_chained_import.lib_ordinal`. Apple `ld64`
    /// reserves `0` for "self" (rebases, not binds), so all
    /// `LC_LOAD_DYLIB`-resolved imports use `≥ 1`.
    pub fn ordinal(self) -> u8 {
        self as u8
    }
}

/// Return the `DyldLib` that owns `name`, or `None` if the symbol
/// isn't part of any known dyld substrate library and must be
/// resolved locally (or surfaced as `UnresolvedExterns`).
///
/// SD-1 covered libSystem only. SD-4b extends this to libcurl,
/// returning `DyldLib::LibCurl` for the small set of `curl_easy_*`
/// symbols torajs-fetch transitively references.
pub fn dyld_lib_for(name: &str) -> Option<DyldLib> {
    if libsystem_set().contains(name) {
        Some(DyldLib::LibSystem)
    } else if libcurl_set().contains(name) {
        Some(DyldLib::LibCurl)
    } else {
        None
    }
}

/// Return `true` iff `name` is a known export of any dyld
/// substrate library. Convenience wrapper over `dyld_lib_for`.
pub fn is_dyld_resolved(name: &str) -> bool {
    dyld_lib_for(name).is_some()
}

/// Back-compat — `true` iff `name` is libSystem-resolved
/// specifically. SD-1 callers used this; SD-4b callers should
/// prefer `dyld_lib_for` to get the lib_ordinal too.
pub fn is_libsystem_resolved(name: &str) -> bool {
    libsystem_set().contains(name)
}

/// Number of distinct libSystem symbols recognised by SD-1.
pub fn libsystem_sym_count() -> usize {
    libsystem_set().len()
}

/// Number of distinct libcurl symbols recognised by SD-4b.
pub fn libcurl_sym_count() -> usize {
    libcurl_set().len()
}

fn libsystem_set() -> &'static BTreeSet<&'static str> {
    static SET: OnceLock<BTreeSet<&'static str>> = OnceLock::new();
    SET.get_or_init(|| LIBSYSTEM_SYMS.iter().copied().collect())
}

fn libcurl_set() -> &'static BTreeSet<&'static str> {
    static SET: OnceLock<BTreeSet<&'static str>> = OnceLock::new();
    SET.get_or_init(|| LIBCURL_SYMS.iter().copied().collect())
}

/// SD-4b — libcurl.4.dylib exports referenced by torajs-fetch.
/// Sourced from `probe_real_link ___torajs_str_alloc` at HEAD
/// `167fc50` after SD-4a print port: the remaining 5 unresolved
/// symbols are all `_curl_easy_*` entries that
/// `libcurl.4.dylib`'s otool -L shows.
const LIBCURL_SYMS: &[&str] = &[
    "_curl_easy_cleanup",
    "_curl_easy_getinfo",
    "_curl_easy_init",
    "_curl_easy_perform",
    "_curl_easy_setopt",
];

/// Static array form — kept sorted so the source diff is reviewable
/// against future probe runs. `libsystem_set()` materialises it into
/// a `BTreeSet` once for O(log N) membership checks.
const LIBSYSTEM_SYMS: &[&str] = &[
    "_CCRandomGenerateBytes",
    "__NSGetArgc",
    "__NSGetArgv",
    "__NSGetEnviron",
    "__NSGetExecutablePath",
    "__Unwind_Backtrace",
    "__Unwind_GetCFA",
    "__Unwind_GetDataRelBase",
    "__Unwind_GetIP",
    "__Unwind_GetIPInfo",
    "__Unwind_GetLanguageSpecificData",
    "__Unwind_GetRegionStart",
    "__Unwind_GetTextRelBase",
    "__Unwind_Resume",
    "__Unwind_SetGR",
    "__Unwind_SetIP",
    "___error",
    "__dyld_get_image_header",
    "__dyld_get_image_name",
    "__dyld_get_image_vmaddr_slide",
    "__dyld_image_count",
    "__exit",
    "__tlv_atexit",
    "__tlv_bootstrap",
    "_abort",
    "_accept",
    "_backtrace",
    "_bind",
    "_calloc",
    "_chdir",
    "_chmod",
    "_chown",
    "_chroot",
    "_clock_gettime",
    "_close",
    "_closedir",
    "_confstr",
    "_connect",
    "_copyfile_state_alloc",
    "_copyfile_state_free",
    "_copyfile_state_get",
    "_dirfd",
    "_dispatch_release",
    "_dispatch_semaphore_create",
    "_dispatch_semaphore_signal",
    "_dispatch_semaphore_wait",
    "_dispatch_time",
    "_dlclose",
    "_dlerror",
    "_dlopen",
    "_dlsym",
    "_dprintf",
    "_dup",
    "_dup2",
    "_execvp",
    "_exit",
    "_fchmod",
    "_fchown",
    "_fclonefileat",
    "_fcntl",
    "_fcopyfile",
    "_fdopendir",
    "_flock",
    "_fork",
    "_free",
    "_freeaddrinfo",
    "_fsetattrlist",
    "_fstat",
    "_fstatat",
    "_ftruncate",
    "_gai_strerror",
    "_getaddrinfo",
    "_getcwd",
    "_getenv",
    "_gethostname",
    "_getpeereid",
    "_getpeername",
    "_getpid",
    "_getppid",
    "_getpwuid_r",
    "_getsockname",
    "_getsockopt",
    "_getuid",
    "_ioctl",
    "_kill",
    "_killpg",
    "_lchown",
    "_linkat",
    "_listen",
    "_lseek",
    "_lstat",
    "_mach_error_string",
    "_mach_timebase_info",
    "_mach_wait_until",
    "_malloc",
    "_mkdir",
    "_mkfifo",
    "_mmap",
    "_mprotect",
    "_munmap",
    "_nanosleep",
    "_open",
    "_openat",
    "_opendir",
    "_pause",
    "_pipe",
    "_poll",
    "_posix_memalign",
    "_posix_spawn_file_actions_adddup2",
    "_posix_spawn_file_actions_destroy",
    "_posix_spawn_file_actions_init",
    "_posix_spawnattr_destroy",
    "_posix_spawnattr_init",
    "_posix_spawnattr_setflags",
    "_posix_spawnattr_setpgroup",
    "_posix_spawnattr_setsigdefault",
    "_posix_spawnp",
    "_pread",
    "_pthread_attr_destroy",
    "_pthread_attr_init",
    "_pthread_attr_setstacksize",
    "_pthread_cond_broadcast",
    "_pthread_cond_destroy",
    "_pthread_cond_signal",
    "_pthread_cond_timedwait_relative_np",
    "_pthread_cond_wait",
    "_pthread_create",
    "_pthread_detach",
    "_pthread_get_stackaddr_np",
    "_pthread_get_stacksize_np",
    "_pthread_join",
    "_pthread_mutex_destroy",
    "_pthread_mutex_init",
    "_pthread_mutex_lock",
    "_pthread_mutex_trylock",
    "_pthread_mutex_unlock",
    "_pthread_mutexattr_destroy",
    "_pthread_mutexattr_init",
    "_pthread_mutexattr_settype",
    "_pthread_self",
    "_pthread_setname_np",
    "_pthread_threadid_np",
    "_pwrite",
    "_read",
    "_readdir_r",
    "_readlink",
    "_readv",
    "_realloc",
    "_realpath$DARWIN_EXTSN",
    "_recv",
    "_recvfrom",
    "_rename",
    "_rmdir",
    "_sched_yield",
    "_send",
    "_sendto",
    "_setattrlist",
    "_setenv",
    "_setgid",
    "_setgroups",
    "_setpgid",
    "_setsid",
    "_setsockopt",
    "_setuid",
    "_shutdown",
    "_sigaction",
    "_sigaddset",
    "_sigaltstack",
    "_sigemptyset",
    "_signal",
    "_snprintf",
    "_socket",
    "_socketpair",
    "_stat",
    "_strerror_r",
    "_strncmp",
    "_strnlen",
    "_symlink",
    "_sysconf",
    "_sysctlbyname",
    "_system",
    "_unlink",
    "_unlinkat",
    "_unsetenv",
    "_waitpid",
    "_write",
    "_writev",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn malloc_family_is_libsystem_resolved() {
        for sym in &["_malloc", "_calloc", "_realloc", "_free", "_posix_memalign"] {
            assert!(is_libsystem_resolved(sym), "{sym} should be libSystem");
        }
    }

    #[test]
    fn pthread_family_is_libsystem_resolved() {
        for sym in &[
            "_pthread_create",
            "_pthread_join",
            "_pthread_self",
            "_pthread_mutex_lock",
            "_pthread_mutex_unlock",
            "_pthread_setname_np",
        ] {
            assert!(is_libsystem_resolved(sym), "{sym} should be libSystem");
        }
    }

    #[test]
    fn unwind_family_is_libsystem_resolved() {
        // macOS folds libunwind into libSystem; otool -L on a
        // production binary confirms no separate libunwind LC.
        for sym in &[
            "__Unwind_Resume",
            "__Unwind_Backtrace",
            "__Unwind_GetIP",
            "__Unwind_SetIP",
        ] {
            assert!(is_libsystem_resolved(sym), "{sym} should be libSystem");
        }
    }

    #[test]
    fn tlv_and_dyld_apple_private_are_libsystem_resolved() {
        for sym in &[
            "__tlv_bootstrap",
            "__tlv_atexit",
            "__dyld_image_count",
            "__dyld_get_image_header",
        ] {
            assert!(is_libsystem_resolved(sym), "{sym} should be libSystem");
        }
    }

    #[test]
    fn syscall_family_is_libsystem_resolved() {
        for sym in &[
            "_open", "_close", "_read", "_write", "_lseek", "_mmap", "_munmap", "_fcntl", "_stat",
            "_fstat",
        ] {
            assert!(is_libsystem_resolved(sym), "{sym} should be libSystem");
        }
    }

    #[test]
    fn curl_family_is_not_libsystem_resolved() {
        // libcurl.4.dylib lives outside libSystem — leaving it
        // unresolved is the intended SD-1 surface; SD-1+ will add
        // a separate LC_LOAD_DYLIB.
        for sym in &[
            "_curl_easy_init",
            "_curl_easy_setopt",
            "_curl_easy_perform",
            "_curl_easy_cleanup",
            "_curl_easy_getinfo",
        ] {
            assert!(
                !is_libsystem_resolved(sym),
                "{sym} must NOT be classified as libSystem"
            );
        }
    }

    #[test]
    fn toolchain_helpers_are_not_libsystem_resolved() {
        // `_print_bool` / `_print_f64` / `_print_i64` are emitted by
        // ssa_inkwell's LLVM IR pipeline today; SD-4 must re-emit
        // them through torajs-codegen. They are NOT dyld symbols.
        for sym in &["_print_bool", "_print_f64", "_print_i64"] {
            assert!(
                !is_libsystem_resolved(sym),
                "{sym} must NOT be classified as libSystem"
            );
        }
    }

    #[test]
    fn unknown_symbols_are_not_libsystem_resolved() {
        for sym in &["___torajs_str_alloc", "_my_user_fn", "", "_"] {
            assert!(!is_libsystem_resolved(sym));
        }
    }

    #[test]
    fn sym_array_is_sorted_for_reviewable_diffs() {
        for w in LIBSYSTEM_SYMS.windows(2) {
            assert!(w[0] < w[1], "{} >= {}; keep array sorted", w[0], w[1]);
        }
    }

    #[test]
    fn sym_array_has_no_duplicates() {
        let set = libsystem_set();
        assert_eq!(set.len(), LIBSYSTEM_SYMS.len());
    }

    #[test]
    fn count_matches_probe_observed_surface() {
        // The probe of `___torajs_str_alloc` against the full
        // 32-archive set at HEAD cd546fa observed ~192 unresolved
        // externs total; subtracting 5 libcurl + 3 toolchain helper
        // leaves the libSystem residue (this assertion guards against
        // accidentally dropping a syscall from the table during
        // future curation).
        assert!(
            libsystem_sym_count() >= 180,
            "count {} dropped below floor — did a syscall family get deleted?",
            libsystem_sym_count()
        );
        assert!(
            libsystem_sym_count() <= 200,
            "count {} above ceiling — non-libSystem leaked in?",
            libsystem_sym_count()
        );
    }
}
