// T-03 (v0.3.0) — process.stderr.write(s)
// Mirror of stdout.write but routed to fd 2. console.log routing
// is unchanged (stays on stdout via inkwell `putchar`); only the
// stderr.write call lands on fd 2. Conformance runner captures
// both streams and compares against bun's joint output, so this
// fixture exercises the stream split too.
let ok1: boolean = process.stderr.write("warning: alpha\n")
let ok2: boolean = process.stderr.write("warning: beta\n")
console.log("stderr writes ok =", ok1, ok2)

// Empty write returns true and emits nothing.
let ok3: boolean = process.stderr.write("")
console.log("empty stderr ok =", ok3)
