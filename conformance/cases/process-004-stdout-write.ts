// T-03 (v0.3.0) — process.stdout.write(s)
// Bun signature: writes raw bytes to stdout (no formatting / no
// trailing newline) and returns boolean. tr panics on short write,
// so the only return that reaches user code is `true`.
let ok1: boolean = process.stdout.write("alpha")
let ok2: boolean = process.stdout.write(" beta")
let ok3: boolean = process.stdout.write(" gamma\n")
console.log("ok =", ok1, ok2, ok3)

// Empty string write — legal, returns true, emits zero bytes.
let ok4: boolean = process.stdout.write("")
console.log("empty ok =", ok4)

// Multi-byte UTF-8 (ascii subset only — wider Unicode under
// `String` is bun-parity but bench-dependent). Verify the byte
// stream survives intact.
process.stdout.write("hello")
process.stdout.write("\n")
