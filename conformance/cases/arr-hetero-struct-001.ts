// Rotation 231 holes X+Y — heterogeneous heap-element array
// literals. The lowering kind probe collapsed every struct into one
// kind and every array into another, so distinct layouts shared the
// anchor's StructId (loud no-field reject on shape mismatch, silent
// garbage on repr mismatch), and a mixed inner array unified into a
// typed outer whose readers mis-strode the tagged slots (5e-323).
//
// Recorded residual (NOT asserted here): a pure-undefined-valued
// struct field (`[{r: 2}, {r: undefined}]` → arr[1].r) answers null
// instead of undefined — the field registers a kind-less Ptr slot
// and the write erases the null/undefined distinction (RFC 20260710
// C2b family; needs the slot upgraded to Any or a Ptr sentinel).

// hole Y loud face: different field sets in one literal
const ab = [{ r: 2 }, { p: 5 }];
console.log(ab[1].p, ab[0].r);

// three-way shape mix, reads across all elements
const tri = [{ a: 1 }, { b: 2, c: 3 }, { a: 9, d: 4 }];
console.log(tri[0].a, tri[1].b, tri[1].c, tri[2].d);

// hole X: mixed inner-array reprs (typed / any / empty)
const xs = [[1], [undefined, 2], []];
console.log(xs[0][0], xs[1][0], xs[1][1], xs[2].length);

// hole X through destructuring assignment (the shape that exposed it)
let x = 0;
let y = 0;
for ([x = 10, y = 20] of [[1], [undefined, 2], []]) {
  console.log(x, y);
}

// hole X through the decl-head pattern
for (const [m = 10, n = 20] of [[1], [undefined, 2], []]) {
  console.log(m, n);
}

// mixed struct fields under for-of patterns (values all present)
for (const { p, r } of [{ p: 1, r: 2 }, { p: 3, r: 4 }]) {
  console.log(p, r);
}

// width-subtyped struct family must STAY on the typed lane
// (prefix-compatible offsets; the checker keeps it typed)
const ws = [{ r: 2 }, { r: 3, s: 4 }];
console.log(ws[0].r, ws[1].r);

// guarded lanes: typed nested arrays and the Str sentinel shape
const tn = [[1, 2], [3, 4]];
console.log(tn[1][0]);
let sv = ["a", undefined];
console.log(sv[0], sv[1]);
