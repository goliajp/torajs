// P10.2-A1 — Promise.resolve() 0-arg form
// ≡ Promise.resolve(undefined) per ES spec §27.2.4.7.
// Inner T = Undefined. Chained .finally proves the substrate
// drains as a real microtask (sync log appears first).
//
// Companion 0-arg Promise.reject() substrate also lands in this
// commit but its runtime-side fixture is deferred to P10.2-A1.1
// (extends .then/.catch to accept Type::Undefined inner T) —
// without that, the only swallow path is .finally, which does
// not silence the rejection. Tested via tr runtime smoke in the
// commit's pre-flight rather than via conformance fixture.

Promise.resolve().finally(() => console.log("done"))
console.log("sync")
