// A top-level binding that comes out of a destructuring pattern is a
// module-level binding like any other: a function DECLARATION reads and
// writes it (it used to be main-local, so `f()` died with a
// ReferenceError while the same read from an arrow worked).
const [a, b] = [1, 2];
const { x, y: yy = 5 } = { x: "s" } as any;
const obj = { p: 10, q: [1, 2, 3] };
const { p, q: [q0, , q2] } = obj;
let [m = 7, ...rest] = [] as number[];
const [[n1], { deep }] = [[1], { deep: true }] as any;
function f() { return a + b; }
function g() { return x + yy + p + q0 + q2 + m + rest.length + n1 + String(deep); }
function bump() { m += 1; return m; }
console.log(f(), g(), bump(), m, a * 2 + b, typeof a, [a, b].map((v) => v + 1).join());
for (const [i, j] of [[1, 2]]) { (function h() { console.log(i + j); })(); }
const [{ k }, arr2, s2 = "dflt"] = [{ k: "v" }, [1, 2]] as any;
const { o1 = { z: 1 }, o2 } = { o2: ["h"] } as any;
function r() { return k + arr2.length + s2 + o1.z + o2[0]; }
function w() { o1.z += 1; return o1.z; }
console.log(r(), w(), o1.z, r());
