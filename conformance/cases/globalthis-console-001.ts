// RFC 20260812-console-sink knife 4 — console as a VALUE: the WHATWG
// §1.1 singleton (five logger cells via fill_ns_methods), identity
// across the bare read, the globalThis fill, the dynamic lane, and
// the Web IDL @@toStringTag badge. stderr bytes compared to bun
// manually in pre-flight. A typed alias's member CALL
// (`const c = console; c.log(...)`) stays the recorded Math-alias
// boundary (`const m = Math; m.max(...)` raises the same loud
// unsupported-member-call-shape) — the dynamic lane below is the
// working escape hatch.
const c = console;
console.log(c === console);
console.log(globalThis.console === console);
console.log(typeof console);
const g: any = globalThis;
g.console.warn("dyn warn stderr");
(console as any).error("dyn err stderr");
console.log(Object.prototype.toString.call(console));
console.log("done");
