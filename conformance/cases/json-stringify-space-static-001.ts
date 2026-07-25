// Rotation 208 — the `space` argument on the STATIC lane. The any
// lane got it one step earlier; a statically shaped receiver kept
// its compile-time unfold and answered the compact text, because
// routing it through the runtime walk would have meant boxing away
// the frontend types that tell an `undefined` field from a null one.
//
// So the unfold splices the indentation itself: it asks the runtime
// for the normalized gap once at the call site, and each nesting
// level's line break, the closing bracket's return to its parent
// level and the colon's trailing space are runtime calls threaded
// through the same concat chain. Depth is a compile-time constant
// there, and an any-typed member carries it into the runtime walk so
// its own indentation continues rather than restarting at zero.
//
// Everything here is a static shape: no `: any` annotations, so a
// compact call still emits exactly the instruction sequence it did
// before — every indent emission sits behind a compile-time gate.

console.log(JSON.stringify({ a: 1, b: { c: [1, 2], d: "x" }, e: [] }, null, 2));
console.log("--");
console.log(JSON.stringify([1, [2, [3]]], null, 2));
console.log("--");

// The step 8.a / 8.b verdicts still hold under a gap — this is the
// pair that made boxing unacceptable.
console.log(JSON.stringify({ u: undefined, k: 1, n: null }, null, 2));
console.log(JSON.stringify([undefined], null, 2));
console.log("--");

// Empty composites stay on one line.
console.log(JSON.stringify([], null, 2), JSON.stringify({}, null, 2));
console.log("--");

// An `any` member continues the static parent's depth.
const anyv: any = { z: [1, { w: 2 }] };
console.log(JSON.stringify({ top: 1, nested: anyv }, null, 2));
console.log("--");

// An empty gap is the compact form, all the way down to the colon
// having no trailing space — and it is only knowable at runtime,
// since a Number space of 0 normalizes to it.
console.log(JSON.stringify({ a: 1 }, null, "\t"));
console.log(JSON.stringify({ a: 1 }, null, 0));
console.log(JSON.stringify({ a: 1 }, null, null));
console.log(JSON.stringify({ a: 1 }));
console.log("--");

// Shapes whose value recursion goes through a hook or a class.
console.log(JSON.stringify({ d: new Date(0), s: "q" }, null, 2));
class P {
  constructor(
    public x: number,
    public y: string,
  ) {}
}
console.log(JSON.stringify(new P(1, "t"), null, 2));
console.log("--");

// A primitive-only layout would take the builder fast path, which
// has nowhere to splice indentation — under a gap it takes the
// concat lane instead.
console.log(JSON.stringify({ a: 1, b: 2, c: true }, null, 4));
