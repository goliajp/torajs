// a closure reads the binding it captures, not a top-level binding of
// the same name initialized from a field read
type F = (x: number) => number;

function m(f: F): number { return f(1) }
let k = 3;
const src = { g: (x: number) => x + k };
const g = src.g;
function twice(x: number) { return x * 2 }
function run() { const g = twice; const t = () => m(g); return t() }
function nested() { const g = twice; const t = () => () => m(g); return t()() }
const top = () => m(g);
console.log(run(), nested(), top(), m(g));
