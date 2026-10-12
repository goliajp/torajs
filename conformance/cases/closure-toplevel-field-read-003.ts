// a top-level binding read off a closure field of a top-level literal
// whose binding is later rebound to another type keeps the closure
let src = { g: (x: number) => x + 1, n: 5 };
const g = src.g;
const h = g;
function call() { return g(41) + h(1) }
function caught() { const t = () => h(2); return t() }
console.log(call(), caught(), typeof src);
src = "gone";
console.log(call(), caught(), typeof src);
