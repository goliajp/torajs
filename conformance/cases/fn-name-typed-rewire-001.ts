// chunk 798 — typed-tier `.name` reads the runtime fn-addr registry
// (not the call-site ident): aliases answer the registered name,
// field receivers answer instead of "", bound closures answer the
// "bound "-prefixed target name.
function g(a: number, b: number): number { return a + b; }
const h = g;
console.log(h.name);
console.log(g.name);
const obj = { f: g };
console.log(obj.f.name);
const arrow = (x: number): number => x;
console.log(arrow.name);
const aliasArrow = arrow;
console.log(aliasArrow.name);
const b1 = g.bind(null);
console.log(b1.name);
console.log(b1);
console.log(b1.length);
const b2 = g.bind(null, 1);
console.log(b2.name);
console.log(b2.length);
function my_fn(x: number): number { return x; }
const b3 = my_fn.bind(null);
console.log(b3.name);
console.log(h.length);
console.log(g.length);
console.log(h(2, 3));
console.log(b1(4, 5));
console.log(b2(6));
console.log(b3(7));
