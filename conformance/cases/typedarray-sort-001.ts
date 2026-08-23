// §23.2.3.29 sort / §23.2.3.34 toSorted for typed arrays.
//
// The default comparator is NUMERIC (§23.2.4.7), not the ToString
// order Array.prototype.sort uses — [10, 9] sorts to [9, 10] here
// and stays [10, 9] there. And it is a total order over values that
// have none: -0 before +0, NaN after everything, all NaNs equal.

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

// numeric, not lexicographic — the difference from Array.sort
console.log(show(new Uint8Array([10, 9, 100, 1]).sort()));
console.log([10, 9, 100, 1].sort().join(","));
console.log(show(new Int32Array([3, -1, 2, -5]).sort()));
console.log(show(new Float64Array([2.5, -0.5, 10, 1]).sort()));

// empty, single, already sorted, reversed, all equal
console.log(show(new Uint8Array(0).sort()), show(new Uint8Array([7]).sort()));
console.log(show(new Uint8Array([1, 2, 3]).sort()));
console.log(show(new Uint8Array([3, 2, 1]).sort()));
console.log(show(new Uint8Array([5, 5, 5, 5]).sort()));

// NaN last, all NaNs equal; -0 before +0
console.log(show(new Float64Array([NaN, 1, NaN, -1]).sort()));
const z = new Float64Array([0, -0, 0, -0]).sort();
console.log(1 / z[0], 1 / z[1], 1 / z[2], 1 / z[3]);
console.log(show(new Float64Array([Infinity, -Infinity, 0, NaN]).sort()));

// a comparator, and only the SIGN of its result is consulted
console.log(show(new Uint8Array([1, 2, 3, 4]).sort((a: any, b: any) => b - a)));
console.log(show(new Uint8Array([1, 2, 3, 4]).sort(() => 0)));
console.log(show(new Uint8Array([3, 1, 2]).sort((a: any, b: any) => (a < b ? -0.5 : 0.5))));
// A comparator with a CONSTANT non-zero sign is inconsistent — it
// claims a < b and b < a for the same pair — and the result is
// therefore not specified. node and bun are exact mirrors of each
// other on it (`sort(() => -999)` is 1,2,3,4 in node and 4,3,2,1 in
// bun, and `sort(() => 999)` swaps them back), so there is nothing
// to be byte-equal to. tr answers what node does, which is what a
// stable merge that only moves on Greater falls out to.
// a NaN result is +0
console.log(show(new Uint8Array([3, 1, 2]).sort(() => NaN)));

// the comparator sees element values, and the sort is stable across
// keys it calls equal
const st = new Uint8Array([5, 1, 6, 2, 7, 3]);
console.log(show(st.sort((a: any, b: any) => (a % 2) - (b % 2))));

// sort returns the receiver; toSorted returns a fresh array
const src = new Uint8Array([3, 1, 2]);
const same = src.sort();
console.log(same === src, show(src));
const src2 = new Uint8Array([3, 1, 2]);
const fresh = src2.toSorted();
console.log(fresh === src2, show(fresh), show(src2), fresh.buffer === src2.buffer);
console.log(show(new Uint8Array([10, 9, 100, 1]).toSorted()));
console.log(show(new Uint8Array([1, 2, 3]).toSorted((a: any, b: any) => b - a)));

// a present non-callable comparator is a TypeError, and it is
// reported before the receiver is even looked at
console.log(attempt(() => new Uint8Array([1]).sort(1 as any)));
console.log(attempt(() => new Uint8Array([1]).sort(null as any)));
console.log(attempt(() => new Uint8Array([1]).toSorted("x" as any)));
console.log(attempt(() => new Uint8Array([1]).sort(undefined as any)));

// BigInt elements order mathematically, not by their bit pattern
console.log(show(new BigInt64Array([3n, -1n, 2n, -5n]).sort()));
console.log(show(new BigUint64Array([3n, 1n, 18446744073709551615n]).sort()));
console.log(show(new BigInt64Array([1n, 2n, 3n]).sort((a: any, b: any) => (a < b ? 1 : -1))));

// a comparator that throws propagates, and does not corrupt the view
const t = new Uint8Array([3, 1, 2]);
console.log(
  attempt(() =>
    t.sort(() => {
      throw new RangeError("nope");
    }),
  ),
);
console.log(t.length);

// float32 sorts what was STORED
console.log(show(new Float32Array([0.3, 0.1, 0.2]).sort()));
