//! macOS aarch64 BSD syscall numbers.
//!
//! Cross-referenced against XNU `bsd/kern/syscalls.master` and
//! `bsd/sys/syscall.h`. On aarch64 the BSD syscall class is
//! implicit in the `svc #0x80` instruction; no `0x2000000` mask
//! needed when `x16` carries the number (unlike the x86_64 path
//! which uses the high bit).
//!
//! Linux + x86_64 macOS sysno tables land in v0.7-A1 follow-up
//! sub-step (cfg-gated arch modules).

/// `exit(int code) -> noreturn` — process termination. Same as
/// the C `_exit(2)` — no atexit handlers, no stdio flush.
pub const SYS_EXIT: u32 = 1;

/// `read(int fd, void *buf, size_t nbyte) -> ssize_t`.
pub const SYS_READ: u32 = 3;

/// `write(int fd, const void *buf, size_t nbyte) -> ssize_t`.
pub const SYS_WRITE: u32 = 4;

/// `open(const char *path, int flags, mode_t mode) -> int`.
pub const SYS_OPEN: u32 = 5;

/// `close(int fd) -> int`.
pub const SYS_CLOSE: u32 = 6;

/// `getpid(void) -> pid_t`.
pub const SYS_GETPID: u32 = 20;

/// `munmap(void *addr, size_t len) -> int`.
pub const SYS_MUNMAP: u32 = 73;

/// `fstat(int fd, struct stat *buf) -> int` (64-bit stat on aarch64).
pub const SYS_FSTAT: u32 = 339;

/// `mmap(void *addr, size_t len, int prot, int flags, int fd, off_t off) -> void*`.
pub const SYS_MMAP: u32 = 197;

/// `lseek(int fd, off_t offset, int whence) -> off_t`.
pub const SYS_LSEEK: u32 = 199;

/// `kill(pid_t pid, int sig) -> int` — used for abort() routing.
pub const SYS_KILL: u32 = 37;

/// `getentropy(void *buf, size_t len) -> int` — kernel CSPRNG fill,
/// `len <= 256`. Used to seed `Math.random` with crypto-quality
/// entropy instead of `SystemTime::now()` (which pulls libc
/// `clock_gettime` / `__error` / `strerror_r` into the user binary).
pub const SYS_GETENTROPY: u32 = 500;

/// Signal numbers (`<sys/signal.h>`, stable BSD/macOS values).
/// `SIGABRT` is delivered by `__torajs_syscall_abort` for orthodox
/// `abort(3)` semantics; `SIGKILL` is the uncatchable escalation if a
/// SIGABRT handler ever swallows the signal and control returns.
pub const SIGABRT: i32 = 6;
pub const SIGKILL: i32 = 9;

/// `__ulock_wait(uint32_t op, void *addr, uint64_t value, uint32_t timeout_us)
/// -> int` — XNU userland sync primitive. Park the calling thread
/// until `*(u32 *)addr != value`. `timeout_us == 0` means no timeout.
/// Used by macOS pthread_mutex / os_unfair_lock internally; calling
/// it directly skips the libc wrapper. v0.7-A5 16-b backs
/// `torajs-mutex::Mutex` lock contended path on macOS.
pub const SYS_ULOCK_WAIT: u32 = 515;

/// `__ulock_wake(uint32_t op, void *addr, uint64_t wake_value) -> int`
/// — wake threads parked on the same `addr`. `wake_value` is opaque
/// for `UL_COMPARE_AND_WAIT`. Returns the wake count on success.
pub const SYS_ULOCK_WAKE: u32 = 516;

/// `__ulock_wait` / `__ulock_wake` operation flag. Wait if `*addr`
/// (read as a 32-bit unsigned integer) equals the `value` arg.
/// Cross-referenced against XNU `bsd/sys/ulock.h` —
/// `UL_COMPARE_AND_WAIT = 1`. Adequate for futex-style mutex
/// implementations; the os_unfair_lock-backed `UL_UNFAIR_LOCK = 2`
/// flavor is XNU-specific and would not map cleanly to Linux futex
/// for the v0.7 cross-platform target.
pub const UL_COMPARE_AND_WAIT: u32 = 1;

/// `ulock_wake` flag — also wake all parked threads on this addr
/// (not just one). Used during unlock-to-final-state transitions
/// to drain the waiter queue.
pub const ULF_WAKE_ALL: u32 = 0x00000100;

/// File-descriptor table sentinels — match libc / POSIX.
pub const STDIN_FD: i32 = 0;
pub const STDOUT_FD: i32 = 1;
pub const STDERR_FD: i32 = 2;

/// `mmap` PROT flags. Mirror `<sys/mman.h>` PROT_READ / PROT_WRITE /
/// PROT_EXEC / PROT_NONE bit masks (stable across macOS versions).
pub const PROT_NONE: i32 = 0x00;
pub const PROT_READ: i32 = 0x01;
pub const PROT_WRITE: i32 = 0x02;
pub const PROT_EXEC: i32 = 0x04;

/// `mmap` MAP flags subset. ANON = "no backing file", PRIVATE =
/// copy-on-write (vs MAP_SHARED). MAP_ANON | MAP_PRIVATE is the
/// canonical "give me fresh zero-filled memory" pattern we'll
/// use for the bump/slab allocator in v0.7-A2.
pub const MAP_PRIVATE: i32 = 0x0002;
pub const MAP_ANON: i32 = 0x1000;

/// `open` flags subset. Mirrors `<fcntl.h>` constants.
pub const O_RDONLY: i32 = 0;
pub const O_WRONLY: i32 = 1;
pub const O_RDWR: i32 = 2;
pub const O_CREAT: i32 = 0x0200;
pub const O_TRUNC: i32 = 0x0400;
pub const O_APPEND: i32 = 0x0008;

/// `lseek` whence values.
pub const SEEK_SET: i32 = 0;
pub const SEEK_CUR: i32 = 1;
pub const SEEK_END: i32 = 2;
