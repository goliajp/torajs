// a top-level binding read off a closure field of a top-level literal
// that a delete, a defineProperty or an unspelled write moves to the
// any lane holds whatever the field held, from named fns too
let a = { g: (x: number) => x + 1, n: 1 };
delete a.g;
const ag = a.g;
let b = { g: (x: number) => x + 2, n: 2 };
const bg = b.g;
delete b.g;
let c = { g: (x: number) => x + 3, n: 3 };
Object.defineProperty(c, "g", { value: 7 });
const cg = c.g;
let d = { g: (x: number) => x + 4, n: 4 };
d.z = "extra";
const dg = d.g;
const dh = dg;

function callA() { try { return ag(1) } catch (e) { return e instanceof TypeError } }
function callC() { try { return cg(1) } catch (e) { return e instanceof TypeError } }
function kinds() { return [typeof ag, typeof bg, typeof cg, typeof dg, typeof dh].join(" ") }
function calls() { return [callA(), bg(1), callC(), dg(1), dh(2)].join(" ") }
function nested() { const t = () => dh(10) + bg(10); return t() }

console.log(kinds());
console.log(calls(), nested());
console.log(typeof b.g, c.g, d.z, a.n + b.n + c.n + d.n);
