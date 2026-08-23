// §23.2.3.26 `%TypedArray%.prototype.set` — one name, two operations.
//
// §23.2.3.26.1 takes another typed array; §23.2.3.26.2 takes
// anything else, reads `length` once and then each index. The second
// one never consults @@iterator, which is the whole reason it cannot
// share the constructor's walk: `new Uint8Array(set)` reads three
// elements out of a Set, `ta.set(set)` reads `length`, finds
// undefined, and stores nothing.

function show(ta: any): string {
  let s = "";
  for (let i = 0; i < ta.length; i++) {
    if (i > 0) s = s + ",";
    s = s + String(ta[i]);
  }
  return s;
}

function attempt(f: () => void): string {
  try {
    f();
    return "ok";
  } catch (e: any) {
    return e.constructor.name;
  }
}

// from another typed array — same kind, offset, exact fit
const t = new Uint8Array(5);
t.set(new Uint8Array([1, 2, 3]));
console.log(show(t));
t.set(new Uint8Array([9, 9]), 3);
console.log(show(t));
t.set(new Uint8Array([7, 7, 7, 7, 7]), 0);
console.log(show(t));

// across element types — the conversion is the element type's, so a
// value that does not fit wraps or rounds the way a store would
const wide = new Int32Array([1, -1, 300, 70000]);
const narrow = new Uint8Array(4);
narrow.set(wide);
console.log(show(narrow));
const back = new Int32Array(4);
back.set(new Uint8Array([1, 2, 3, 4]));
console.log(show(back));
const f32 = new Float32Array(3);
f32.set(new Float64Array([0.1, 1.5, -0]));
console.log(show(f32));
const clamped = new Uint8ClampedArray(3);
clamped.set(new Float64Array([-5, 2.5, 300]));
console.log(show(clamped));

// from a plain array and an array-like
const p = new Uint8Array(4);
p.set([1, 2]);
console.log(show(p));
p.set({ length: 2, 0: 8, 1: 9 } as any, 2);
console.log(show(p));
// an object with no `length` sets nothing rather than throwing
const q = new Uint8Array([1, 2, 3]);
q.set({} as any);
console.log(show(q));
// a shorter `length` than the object has keys
const r = new Uint8Array([0, 0, 0]);
r.set({ length: 1, 0: 5, 1: 6 } as any);
console.log(show(r));
// holes and non-numbers coerce the way a store does
const h = new Uint8Array(4);
h.set([1, undefined, "3", null] as any);
console.log(show(h));

// a Set is NOT an array-like — @@iterator is never consulted
const s = new Uint8Array([1, 2, 3]);
s.set(new Set([7, 8, 9]) as any);
console.log(show(s), show(new Uint8Array(new Set([7, 8, 9]) as any)));

// offsets that do not fit are RangeErrors, checked before anything moves
const g = new Uint8Array([1, 2, 3]);
console.log(attempt(() => g.set(new Uint8Array([1, 2]), 2)));
console.log(attempt(() => g.set([1, 2], 2)));
console.log(attempt(() => g.set(new Uint8Array([1]), -1)));
console.log(attempt(() => g.set(new Uint8Array([1]), Infinity)));
console.log(show(g));

// content types do not convert across
console.log(attempt(() => new Uint8Array(2).set(new BigInt64Array([1n]))));
console.log(attempt(() => new BigInt64Array(2).set(new Uint8Array([1]))));
console.log(attempt(() => new BigInt64Array(2).set([1] as any)));
const bi = new BigInt64Array(2);
bi.set(new BigUint64Array([5n]));
console.log(show(bi));
bi.set([7n] as any, 1);
console.log(show(bi));

// two views on ONE buffer: the copy must behave as if the source
// were read out first, in both directions of overlap
const buf = new ArrayBuffer(8);
const v = new Uint8Array(buf);
v.set([1, 2, 3, 4, 5, 6, 7, 8]);
v.set(v.subarray(0, 4), 2);
console.log(show(v));
const buf2 = new ArrayBuffer(8);
const v2 = new Uint8Array(buf2);
v2.set([1, 2, 3, 4, 5, 6, 7, 8]);
v2.set(v2.subarray(4), 2);
console.log(show(v2));
// and across strides on the same buffer
const buf3 = new ArrayBuffer(8);
const b8 = new Uint8Array(buf3);
b8.set([1, 2, 3, 4, 5, 6, 7, 8]);
const b16 = new Uint16Array(buf3);
b8.set(b16.subarray(0, 3), 1);
console.log(show(b8));

// set answers undefined
console.log(new Uint8Array(2).set([1]));
