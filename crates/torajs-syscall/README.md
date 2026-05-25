# torajs-syscall

**0-dep raw syscall stubs for torajs user binary.**

Layer-0 substrate of the [v0.7 Metal](../../docs/roadmap.md) version
— direct OS syscalls so the AOT-emitted user binary can avoid
linking `libSystem.dylib` / `libc.so` entirely.

## Status (v0.7-A1)

- [ ] step 1 — crate scaffold (this commit)
- [ ] step 2 — macOS aarch64 sysno table
- [ ] step 3 — aarch64 `svc` trampoline (`syscall6`)
- [ ] step 4 — safe wrappers (`write` / `read` / `exit` / `mmap` /
      `munmap`)
- [ ] step 5 — `perf_gate.rs` (4× headroom over raw asm)

## Why "metal"

Apple's documented stance is that the Mach kernel syscall ABI is
not a stable public contract; the supported entry is via
`libSystem.dylib`. torajs walks past this disclaimer intentionally
— vision #4 "0 deps" in its full form treats libSystem as a
third-party dep we don't want to ship against.

The result is a user binary that:
- runs without `libSystem.dylib` linked (`otool -L` shows no
  `/usr/lib/libSystem.B.dylib`)
- is portable across XNU minor versions only as far as the syscall
  numbers are stable in practice (Apple doesn't guarantee this,
  but the dominant set has been frozen for years)

## License

Apache-2.0 OR MIT.
