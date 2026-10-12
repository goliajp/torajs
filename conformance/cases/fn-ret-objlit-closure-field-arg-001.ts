// a capturing closure field read off a returned object literal reaches
// a fn-typed parameter as a closure cell
type F = (x: number) => number;

function m(f: (x: number) => number) { return f(1) }
function mk(k: number) { return { g: (x: number) => x + k } }
function mkTyped(k: number): { g: F } { return { g: (x: number) => x * k } }
function run() { return m(mk(7).g) + m(mkTyped(3).g) }

console.log(m(mk(5).g));
const o = mk(2);
console.log(m(o.g), run(), [mk(1), mk(4)].map((v) => m(v.g)).join(","));
