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

/// `unlink(const char *path) -> int` — remove a file. SDK
/// `<sys/syscall.h>` SYS_unlink == 10.
pub const SYS_UNLINK: u32 = 10;

/// `mkdir(const char *path, mode_t mode) -> int`. SYS_mkdir == 136.
pub const SYS_MKDIR: u32 = 136;

/// `rmdir(const char *path) -> int` — remove an empty directory.
/// SYS_rmdir == 137. Companion to `mkdir`; also backs `rmdirSync`.
pub const SYS_RMDIR: u32 = 137;

/// `stat64(const char *path, struct stat *buf) -> int` — path-based
/// stat in the 64-bit-inode layout matching `SYS_FSTAT` (fstat64).
/// SYS_stat64 == 338.
pub const SYS_STAT64: u32 = 338;

/// `getdirentries64(int fd, void *buf, size_t bufsize, off_t *basep)
/// -> ssize_t` — read directory entries in the 64-bit-inode
/// `struct dirent` layout. SYS_getdirentries64 == 344.
pub const SYS_GETDIRENTRIES64: u32 = 344;

/// `mmap(void *addr, size_t len, int prot, int flags, int fd, off_t off) -> void*`.
pub const SYS_MMAP: u32 = 197;

/// `lseek(int fd, off_t offset, int whence) -> off_t`.
pub const SYS_LSEEK: u32 = 199;

/// `fcntl(int fd, int cmd, ...) -> int` — file-descriptor control.
/// SYS_fcntl == 92. macOS `getcwd(3)` libc impl is
/// `open(".", O_RDONLY) + fcntl(fd, F_GETPATH, buf) + close(fd)` —
/// XNU has no dedicated `getcwd` syscall, so the fcntl route is the
/// orthodox way to get the cwd without a libc dependency.
pub const SYS_FCNTL: u32 = 92;

/// `kill(pid_t pid, int sig) -> int` — used for abort() routing.
pub const SYS_KILL: u32 = 37;

/// `getentropy(void *buf, size_t len) -> int` — kernel CSPRNG fill,
/// `len <= 256`. Used to seed `Math.random` with crypto-quality
/// entropy instead of `SystemTime::now()` (which pulls libc
/// `clock_gettime` / `__error` / `strerror_r` into the user binary).
pub const SYS_GETENTROPY: u32 = 500;

/// `gettimeofday(struct timeval *tp, struct timezone *tzp,
/// uint64_t *mach_absolute_time) -> int` — wall-clock time since the
/// UNIX epoch. Cross-referenced against the SDK
/// `<sys/syscall.h>` (`SYS_gettimeofday == 116`). Used as the metal
/// time source for `Date.now()` / `new Date()`, replacing
/// `SystemTime::now()` — macOS has no `clock_gettime` syscall, so std
/// routes it through the libc commpage wrapper, dragging
/// `clock_gettime` / `__error` / `strerror_r` into the user binary.
pub const SYS_GETTIMEOFDAY: u32 = 116;

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

/// `fcntl` cmd: return the absolute path of the fd's underlying
/// vnode (NUL-terminated, fills caller's buffer). Cross-referenced
/// against the SDK `<sys/fcntl.h>` — `F_GETPATH == 50`. Caller buffer
/// must be at least `PATH_MAX` (1024 on darwin; we pass 4096 to
/// match the runtime path buffer everywhere else).
pub const F_GETPATH: i32 = 50;

/// `mkdir` default mode — 0o777 pre-umask, matching
/// `std::fs::create_dir`. The kernel masks it by the process umask
/// (typically 0o022, yielding 0o755 on disk).
pub const MKDIR_DEFAULT_MODE: i32 = 0o777;

/// `struct stat` (64-bit-inode, aarch64 macOS) byte layout — verified
/// via `offsetof` against the SDK `<sys/stat.h>`: `sizeof == 144`,
/// `offsetof(st_size) == 96` (`off_t`, i64). We read only `st_size`,
/// so a fixed byte buffer + named offset is cleaner than mirroring all
/// 17 fields (cf. the small `Timeval` struct, which is worth mapping).
pub const STAT_BUF_SIZE: usize = 144;
pub const STAT_ST_SIZE_OFFSET: usize = 96;

/// `struct dirent` (64-bit-inode) byte layout — verified via `offsetof`
/// against the SDK `<dirent.h>`: d_ino@0 (u64), d_seekoff@8 (u64),
/// d_reclen@16 (u16), d_namlen@18 (u16), d_type@20 (u8), d_name@21
/// (char[]). `getdirentries64` packs variable-length records
/// back-to-back; step by `d_reclen`.
pub const DIRENT_D_RECLEN_OFFSET: usize = 16;
pub const DIRENT_D_NAMLEN_OFFSET: usize = 18;
pub const DIRENT_D_TYPE_OFFSET: usize = 20;
pub const DIRENT_D_NAME_OFFSET: usize = 21;

/// `struct dirent` `d_type` value for a subdirectory entry.
pub const DT_DIR: u8 = 4;
