// Array custom-property printing — `console.log(arr)` must list the
// side-table props after the elements, insertion-ordered, bun-shaped:
// `[ 1, 2, x: 5 ]`. Backed by the dynobj insertion-order rebuild
// (dense entry array + hash index) + torajs-arr print_props.
//
// Faces covered:
// - scalar values: i64 / str (quoted) / bool / f64 / undefined / null
//   (undefined requires the lower_to_tag_value pair path — the plain
//   extractor collapsed it to null)
// - str-element array props (per-type printer dispatch)
// - empty array stays `[]` even with props (bun ground truth)
// - nested dynobj prop: multi-line block, 2-level deep, trailing
//   commas, then back to inline props after the block
// - empty nested dynobj prints `{}`

let a = [1, 2];
a.x = 5;
a.s = "hi";
a.b = true;
a.f = 2.5;
a.u = undefined;
a.n = null;
console.log(a);

let strs = ["p", "q"];
strs.tag = "t";
console.log(strs);

let e: number[] = [];
e.k = 7;
console.log(e);

let nested = [9];
const inner: any = {};
inner.y = "b";
inner.z = 3;
const deep: any = {};
deep.w = 1;
inner.d = deep;
nested.g = inner;
nested.after = 7;
console.log(nested);

let m = ["s"];
const empty: any = {};
m.obj = empty;
console.log(m);
