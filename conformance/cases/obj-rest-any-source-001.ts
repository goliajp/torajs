// Object rest whose source is `any` — the shape a real program hits
// whenever the source came back from a call. It has no static field
// list, so the copy is a run-time CopyDataProperties walk (§7.3.25)
// rather than a compile-time unfold.

const src: any = { p: 1, q: 2, r: 3 };

const { p, ...rest } = src;
console.log(p, JSON.stringify(rest));

let a: any, tail: any;
({ q: a, ...tail } = src);
console.log(a, JSON.stringify(tail));

// §7.3.25 step 3.b — an excluded key is skipped BEFORE [[Get]], so the
// rest copy must not run its getter. The binding itself still reads it
// once, which is the single "getter" line below.
const withGetter: any = {
  get g() {
    console.log("getter");
    return 1;
  },
  x: 2,
};
const { g, ...others } = withGetter;
console.log(g, JSON.stringify(others));

// nothing is excluded from a plain spread, so the getter runs there
const all = { ...withGetter };
console.log(JSON.stringify(all));

// several excluded keys
const wide: any = { k1: 1, k2: 2, k3: 3, k4: 4 };
const { k1, k3, ...remainder } = wide;
console.log(k1, k3, JSON.stringify(remainder));
