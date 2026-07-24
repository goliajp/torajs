// Rotation 207 — ES §25.5.2.4 step 4: a primitive wrapper object
// serializes as the primitive it wraps, not as an ordinary object.
// The any-lane JSON walk had arms for Str / Arr / DynObj / Obj / Date
// / Closure and a catch-all `{}` for the classes with no own
// enumerable properties (Map / Set / RegExp / Promise) — the wrapper
// cells fell into that catch-all, so every one of them answered `{}`.

console.log("A", JSON.stringify(new Number(8.5)));
console.log("B", JSON.stringify(new String("hi")));
console.log("C", JSON.stringify(new Boolean(true)));
console.log("D", JSON.stringify(new Boolean(false)));

// As property values and array elements.
console.log("E", JSON.stringify({ a: new Number(1), b: new String("s"), c: new Boolean(false) }));
console.log("F", JSON.stringify([new Number(2), new String("t"), new Boolean(true)]));

// Step 9 — a non-finite [[NumberData]] is null, same as a bare one.
console.log("G", JSON.stringify(new Number(NaN)));
console.log("H", JSON.stringify(new Number(Infinity)));
console.log("I", JSON.stringify(new Number(-Infinity)));
console.log("J", JSON.stringify(NaN));

// Empty and quote-bearing [[StringData]].
console.log("K", JSON.stringify(new String("")));
console.log("L", JSON.stringify(new String('a"b')));

// Integral doubles keep JSON's number spelling.
console.log("M", JSON.stringify(new Number(0)));
console.log("N", JSON.stringify(new Number(-0)));
console.log("O", JSON.stringify(new Number(1e21)));

// The classes that legitimately answer `{}` still reach the
// catch-all these arms sit in front of. (Typed as `any` — a
// statically Map/Set/RegExp-typed receiver is the struct lane, which
// rejects them outright; that is a separate gap.)
const m: any = new Map();
const s2: any = new Set();
const re: any = /re/g;
console.log("P", JSON.stringify(m));
console.log("Q", JSON.stringify(s2));
console.log("R", JSON.stringify(re));

// Bare primitives are unaffected.
console.log("S", JSON.stringify(8.5));
console.log("T", JSON.stringify("hi"));
console.log("U", JSON.stringify(true));
