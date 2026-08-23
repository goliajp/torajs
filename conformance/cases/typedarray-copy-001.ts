// §23.2.3 slab A copy half — subarray / slice / toReversed / with.
//
// The line that matters: `subarray` mints another VIEW over the same
// bytes, the other three ALLOCATE. So a write through a subarray is
// visible through its source and a write through a slice is not.
//
// `subarray` is also the only member of the slab that does not
// validate — §23.2.3.28 asks for the internal slot and then takes an
// out-of-bounds source length as 0, where `slice` on the same view
// throws.

function show(ta: any): string {
  let s = "";
  for (let i = 0; i < ta.length; i++) {
    if (i > 0) s = s + ",";
    s = s + String(ta[i]);
  }
  return s;
}

const a = new Uint8Array([10, 20, 30, 40, 50]);

// subarray — ranges, negatives, empty, clamped
console.log(show(a.subarray(1)), show(a.subarray(1, 3)), show(a.subarray(-2)));
console.log(show(a.subarray(3, 1)), show(a.subarray(0, 0)), show(a.subarray(-99, 99)));
console.log(a.subarray(1).length, a.subarray(1).byteOffset, a.subarray(1).byteLength);

// a subarray SHARES the bytes
const sub = a.subarray(1, 3);
sub[0] = 99;
console.log(a[1], sub[0], show(a));
a[2] = 88;
console.log(sub[1], show(sub));

// a slice does NOT
const sl = a.slice(1, 3);
sl[0] = 1;
console.log(a[1], sl[0], show(sl));
console.log(show(a.slice()), show(a.slice(2)), show(a.slice(-2)), show(a.slice(3, 1)));
console.log(a.slice(1, 3).buffer === a.buffer, a.subarray(1, 3).buffer === a.buffer);

// element type and byte offsets travel with a subarray
const buf = new ArrayBuffer(16);
const u16 = new Uint16Array(buf, 4, 4);
console.log(u16.length, u16.byteOffset, u16.byteLength);
const s16 = u16.subarray(1, 3);
console.log(s16.length, s16.byteOffset, s16.byteLength, s16.buffer === buf);

// toReversed copies, and leaves the source alone
const r = new Uint8Array([1, 2, 3, 4]);
const rr = r.toReversed();
console.log(show(rr), show(r), rr === r, rr.buffer === r.buffer);
console.log(show(new Uint8Array(0).toReversed()), show(new Uint8Array([7]).toReversed()));
console.log(show(new Float64Array([1.5, NaN, -0]).toReversed()));

// with replaces one element into a copy
const w = new Uint8Array([1, 2, 3]);
console.log(show(w.with(1, 9)), show(w), show(w.with(-1, 9)), show(w.with(0, 300)));
console.log(show(new Uint8ClampedArray([0, 0]).with(0, 2.5)));
console.log(show(new BigInt64Array([1n, 2n]).with(1, -5n)));

// with rejects an index the view does not have — RangeError, and it
// is raised after the value has been coerced
let caught = "";
try {
  w.with(3, 0);
} catch (e: any) {
  caught = e.constructor.name;
}
console.log(caught);
caught = "";
try {
  w.with(-4, 0);
} catch (e: any) {
  caught = e.constructor.name;
}
console.log(caught);

// a length-tracking subarray of a resizable buffer STAYS tracking;
// one given an explicit end is pinned
const rab = new ArrayBuffer(8, { maxByteLength: 16 });
const t = new Uint8Array(rab);
const tracking = t.subarray(2);
const pinned = t.subarray(2, 4);
console.log(t.length, tracking.length, pinned.length);
rab.resize(12);
console.log(t.length, tracking.length, pinned.length);
rab.resize(4);
console.log(t.length, tracking.length, pinned.length);
