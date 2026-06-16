// RFC `20260613-object-property-descriptors` C4 — spec §10.1.6.3
// step 1 receiver guard. `Object.defineProperty / defineProperties /
// getOwnPropertyDescriptor` on a non-object receiver (undefined,
// null, primitive number / string / bool) throws a TypeError per
// spec §20.1.2.4 / §20.1.2.10.
//
// Already wired in code (`ssa_lower_object_define::emit_receiver_typecheck`
// + `ssa_lower.rs` getOwnPropertyDescriptor arm boxes a non-Any receiver
// and lets the runtime helper throw). This fixture is the close-mark
// acceptance — bun-parity for the four byte-equal message shapes plus
// the spec NO-THROW path for number receivers under
// getOwnPropertyDescriptor (auto-wrap to Number wrapper, no throw).
//
// The two `getOwnPropertyDescriptor(undefined|null, …)` cases also
// throw on both sides, but bun appends a ` (evaluating 'expr')` suffix
// the tora runtime helper doesn't synthesize — covered by the
// L3b follow-up "match bun's `(evaluating …)` suffix on TypeError
// messages", not part of this trunk.

// 1. defineProperty(undefined, …) — throws.
try {
    Object.defineProperty(undefined, 'x', { value: 1 });
    console.log("1 NO THROW");
} catch (e) {
    console.log("1 threw:", (e as Error).message);
}

// 2. defineProperty(null, …) — throws.
try {
    Object.defineProperty(null, 'x', { value: 1 });
    console.log("2 NO THROW");
} catch (e) {
    console.log("2 threw:", (e as Error).message);
}

// 3. defineProperty(42, …) — throws (primitive number receiver).
try {
    Object.defineProperty(42, 'x', { value: 1 });
    console.log("3 NO THROW");
} catch (e) {
    console.log("3 threw:", (e as Error).message);
}

// 4. defineProperties(undefined, {…}) — same guard, throws.
try {
    Object.defineProperties(undefined, { x: { value: 1 } });
    console.log("4 NO THROW");
} catch (e) {
    console.log("4 threw:", (e as Error).message);
}

// 5. getOwnPropertyDescriptor(42, 'x') — NO THROW per spec
//    (§20.1.2.10 → ToObject(42) auto-wraps; "x" not own, returns
//    undefined → tr / bun both print `undefined`).
let d5 = Object.getOwnPropertyDescriptor(42, 'x');
console.log("5 desc:", d5);

// 6. Real object — regression guard for the success path. Descriptor
//    fields match bun's enumerable / configurable / writable defaults
//    for a data property created via assignment (all true).
let o: any = { a: 1 };
let d: any = Object.getOwnPropertyDescriptor(o, 'a');
console.log("6 value:", d.value);
console.log("6 writable:", d.writable);
console.log("6 enumerable:", d.enumerable);
console.log("6 configurable:", d.configurable);
