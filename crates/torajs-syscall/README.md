# torajs-syscall

**0-dep raw syscall stubs for torajs user binary.**

Direct OS syscalls so the AOT-emitted user binary can avoid linking
`libSystem.dylib` / `libc.so` entirely. Currently covers aarch64
macOS.

## Why "metal"

Apple's documented stance is that the Mach kernel syscall ABI is
not a stable public contract; the supported entry is via
`libSystem.dylib`. torajs walks past this disclaimer intentionally,
to avoid shipping against libSystem as a dependency.

The result is a user binary that:
- runs without `libSystem.dylib` linked (`otool -L` shows no
  `/usr/lib/libSystem.B.dylib`)
- is portable across XNU minor versions only as far as the syscall
  numbers are stable in practice (Apple doesn't guarantee this,
  but the dominant set has been frozen for years)

## License

Apache-2.0 OR MIT.
