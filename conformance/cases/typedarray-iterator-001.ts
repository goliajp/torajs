// §23.2.5 %TypedArray%.prototype's iterator face — `values` /
// `keys` / `entries`, and the `[Symbol.iterator]` that IS `values`.
//
// §23.2.5.1 does not mint a new kind of iterator: it returns the
// SAME Array Iterator an array does, and CreateArrayIterator's own
// closure branches on whether the source has a [[TypedArrayName]].
// So the cell is `torajs_arr`'s, and only its step knows the two
// things that differ — the length comes from ValidateTypedArray, and
// it is asked fresh on EVERY step rather than once.
//
// That per-step re-ask is the whole reason a for-of over a view can
// notice something an array's cannot.

const ta: any = new Int8Array([10, 20, 30]);

const ofs: any[] = [];
for (const v of ta) {
  ofs.push(v);
}
console.log("for-of", ofs.join(","));
console.log("spread", [...ta].join(","));

const ks: any[] = [];
for (const k of ta.keys()) {
  ks.push(k);
}
console.log("keys", ks.join(","));

const vs: any[] = [];
for (const v of ta.values()) {
  vs.push(v);
}
console.log("values", vs.join(","));

const es: any[] = [];
for (const e of ta.entries()) {
  es.push(e[0] + "=" + e[1]);
}
console.log("entries", es.join(" "));

// §23.2.3.36 — `[Symbol.iterator]` is the same function object as
// `values`, so reading both must land on one cell.
console.log("symiter-is-values", ta[Symbol.iterator] === ta.values);
const it: any = ta[Symbol.iterator]();
console.log("symiter-step", it.next().value, it.next().value, it.next().done);

// The iterator cell is a %Iterator.prototype% citizen like any
// other, so it iterates itself.
const it2: any = ta.values();
console.log("iter-self", it2[Symbol.iterator]() === it2);

// Exhaustion latches: a spent iterator keeps answering done.
const it3: any = new Int8Array([1]).values();
console.log("latch", it3.next().value, it3.next().done, it3.next().done);

// Array.from and destructuring ride the same protocol.
console.log("from", Array.from(ta).join(","));
const [a, b] = ta;
console.log("destr", a, b);

// The method reads themselves are values.
console.log(
  "reads",
  typeof ta.values,
  typeof ta.keys,
  typeof ta.entries,
  typeof ta.forEach,
  typeof ta.at,
);

// BigInt views iterate the same way; each element read mints a cell.
const big: any = new BigInt64Array([7n, 8n]);
const bs: any[] = [];
for (const v of big) {
  bs.push(v);
}
console.log("big-for-of", bs.join(","));
const bes: any[] = [];
for (const e of big.entries()) {
  bes.push(e[0] + "=" + e[1]);
}
console.log("big-entries", bes.join(" "));

// An empty view iterates zero times rather than refusing.
const zero: any = new Uint8Array(0);
let n = 0;
for (const v of zero) {
  n = n + 1;
}
console.log("empty", n, [...zero].length, [...zero.keys()].length);

// A view over a slice of a buffer iterates its own window only.
const buf: any = new ArrayBuffer(8);
const whole: any = new Uint8Array(buf);
whole[0] = 1;
whole[1] = 2;
whole[2] = 3;
whole[3] = 4;
const win: any = new Uint8Array(buf, 1, 2);
console.log("window", [...win].join(","), [...win.entries()].length);
