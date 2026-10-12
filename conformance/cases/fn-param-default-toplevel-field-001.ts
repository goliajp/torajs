// a parameter default reads a top-level binding that holds a closure
// field of a top-level object literal
type F = (x: number) => number;

const p = { g: (x: number) => x * 3 };
const g = p.g;

function t(h: F = g) { return h(2) }
function u(h: F = p.g, n: number = g(5)) { return h(n) }
const w = (h: F = g) => h(7);

console.log(t(), t((x) => x - 1), u(), u(undefined, 1), w());
