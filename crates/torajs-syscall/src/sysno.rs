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
