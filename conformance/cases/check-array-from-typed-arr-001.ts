// S132 narrow — `Array.from(<typed Array<T>>)` shallow copy.
// Pre-S132: check.rs's Array.from sig was fixed to `(String) →
// Array<String>` (for the arrayLike-from-string lowering), rejecting
// any typed Array<T> input with "argument 0: expected String, got
// Array(Number)". This commit adds a Call-level polymorphic dispatch
// (analogous to Object.values's heterogeneous path) that returns
// Array<T> for an Array<T> input, and emits arr_slice + per-element
// rc_inc — the same deep-clone pattern Object.values's Array arm uses.

// number[]
const ns: number[] = [1, 2, 3, 4];
const cn = Array.from(ns);
console.log("cn.len", cn.length);
console.log(cn);
// independence: mutating the copy doesn't affect the source
cn[0] = 99;
console.log("ns[0]", ns[0]);
console.log("cn[0]", cn[0]);

// string[] (refcounted-elem path exercises rc_inc range)
const ss: string[] = ["alpha", "bravo", "charlie"];
const cs = Array.from(ss);
console.log("cs.len", cs.length);
console.log(cs);
cs.push("delta");
console.log("ss.len", ss.length);
console.log("cs.len", cs.length);

// boolean[]
const bs: boolean[] = [true, false, true];
const cb = Array.from(bs);
console.log(cb);

// empty array
const es: number[] = [];
const ce = Array.from(es);
console.log("ce.len", ce.length);

// regression — Array.from(string) still works (original arm)
const fs = Array.from("xyz");
console.log("fs.len", fs.length);
console.log(fs);
