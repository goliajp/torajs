// Coercing an element to BigInt takes a SHARE of the value's cell,
// not ownership of it.
//
// §7.1.13 ToBigInt hands back an owned stake, and for a value that is
// already a BigInt that stake is a `+1` on the CALLER'S cell rather
// than a fresh mint. The element coercion released it with the
// unconditional `__torajs_bigint_drop` — which ignores the refcount
// and frees — so it destroyed a cell the source still held.
//
// The two shapes below are the same bug seen from opposite ends.
// Building a view from an array-like reads every element and coerces
// it, so the SOURCE array's cells died under it: a second
// construction over the same array read a recycled slot and reported
// "Failed to parse String to BigInt". And a walk that keeps an
// element alive across a user callback (findLast returning the value
// it stopped on) handed back memory that had already gone back.

const arr: any[] = [1n, 2n, 3n];

const a: any = new BigInt64Array(arr);
console.log("first", a[0], a[1], a[2]);
console.log("source-after", arr.length, arr[0], arr[1], arr[2], typeof arr[0]);

// The one that used to read a recycled slot.
const b: any = new BigInt64Array(arr);
console.log("second", b[0], b[1], b[2]);
const c: any = new BigInt64Array(arr);
console.log("third", c[0], c[2]);

// Unsigned view of the same source, and a single-element source
// (the shrunk repro).
const u: any = new BigUint64Array(arr);
console.log("unsigned", u[0], u[2]);
const one: any = [5n];
const o1: any = new BigInt64Array(one);
const o2: any = new BigInt64Array(one);
console.log("one-elem", o1[0], o2[0], one[0]);

// Same in a function scope — nothing here depends on the source
// being a promoted top-level global.
function local(): void {
  const src: any[] = [9n, 8n];
  const x: any = new BigInt64Array(src);
  const y: any = new BigInt64Array(src);
  console.log("local", x[0], y[0], y[1], src[0]);
}
local();

// The walk end: findLast returns the element it stopped on, so that
// element has to survive the callback that ran on it.
const s1: any = new BigInt64Array(arr);
console.log(
  "findLast",
  s1.findLast(function (): any {
    return true;
  }),
);

const s2: any = new BigInt64Array(arr);
console.log(
  "findLast-write",
  s2.findLast(function (val: any, i: any): any {
    if (i === 2) {
      s2[0] = 7n;
    }
    return val === 7n;
  }),
);

const s3: any = new BigInt64Array(arr);
console.log(
  "find",
  s3.find(function (v: any): any {
    return v === 2n;
  }),
);

// Writing through a view coerces too, and the value written must
// outlive the write.
const w: any = new BigInt64Array(2);
const big: any = 42n;
w[0] = big;
w[1] = big;
console.log("write-share", w[0], w[1], big, typeof big);

// The source array is still intact after all of it.
console.log("end", arr[0], arr[1], arr[2], one[0]);
