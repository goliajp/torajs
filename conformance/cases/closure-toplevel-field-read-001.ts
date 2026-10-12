// a top-level binding initialized from a closure field of a top-level
// object literal is readable from named fns and the closures inside them
type F = (x: number) => number;

let src = { g: (x: number) => x + 1, n: 5 };
const g = src.g;
const h = g;
const typed: F = src.g;
let k = 3;
const cap = { f: (x: number) => x + k };
const kf = cap.f;

function direct() { return g(1) + h(2) + typed(3) }
function caught() { const t = () => g(1); return t() }
function nested() { function inner() { return h(10) } return inner() }
function viaValue() { const t = () => g; return t()(4) }
function loop() { let s = 0; for (let i = 0; i < 1000; i++) s += g(i); return s + src.n }
function captured() { k = 10; return kf(1) }

console.log(direct(), caught(), nested(), viaValue(), loop(), captured());
src.g = (x: number) => x * 100;
console.log(g(1), src.g(1), typed(1));
