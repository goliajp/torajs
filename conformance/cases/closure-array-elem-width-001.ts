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
