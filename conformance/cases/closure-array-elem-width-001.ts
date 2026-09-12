// 638-13 — a closure stored into a fn-typed array element
// (`(() => number)[]`, via push / literal / index assignment) that
// returns an f64 floats the element signature's return: the call reads
// the float lane. Before: the element signature never widened and
// `fs[0]()` read the caller-saved integer register (a pointer-sized
// garbage number).
const nums = [1, 2, 3];
const fs: (() => number)[] = [];
for (const z of nums) { fs.push(() => z); }
console.log("push", fs[0](), fs[1](), fs[2]());
const hs: (() => number)[] = [];
for (const z of nums) { const k = 5; hs.push(() => k + z); }
console.log("capture2", hs[0](), hs[1](), hs[2]());
const ls: (() => number)[] = [() => 1.5, () => 2];
console.log("literal", ls[0](), ls[1]());
const ms: (() => number)[] = [];
ms.push(() => 0);
ms[0] = () => nums[1] / 2;
console.log("index", ms[0]());
const one = fs[0];
console.log("held", one(), fs.length, typeof fs[0]);

// A method PRODUCT read in place — `cs.concat([...])[1](3)` — has no
// binding whose init resolves its container key, and that resolution
// is what joins the argument literal's element class onto the
// receiver's. Before: the closure in the literal compiled with integer
// faces while the product (the receiver's layout, floated by
// `takeSecond`'s `n * 2`) was called with float ones — the call
// answered its own argument. The let / annotated / literal forms
// resolved through their bindings and were already right.
function takeSecond(fs: ((n: number) => number)[]): number { return fs[1](3); }
console.log("second", takeSecond([(n) => n, (n) => n * 2]));
const cs: ((n: number) => number)[] = [(n) => n];
console.log("bare", cs.concat([(n) => n + 1])[1](3));
const p = cs.concat([(n) => n + 1]);
console.log("let", p[1](3));
const q: ((n: number) => number)[] = cs.concat([(n) => n + 1]);
console.log("annotated", q[1](3));
const only = [(n: number) => n + 1];
console.log("direct-literal", only[0](3), typeof cs.concat([(n) => n + 1]));
const cs2: ((n: number) => number)[] = [];
console.log("concat-onto-empty", cs2.concat([(n) => n + 2])[0](3));
