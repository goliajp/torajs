// A plain function's `P | null` parameter is a struct slot in the
// boxed adapter (the null rides the pointer in band), so a call
// through `any` with an argument that does not fit the layout has to
// reach the all-any twin exactly as a bare `P` parameter does. The
// typed calls, including the literal `null`, keep the typed body.
type P = { x: number; y: number };
type Q = P;

function f(p: P | null): string { return p ? "f x=" + p.x : "f nil"; }
function g(p: Q | null, tag: string): string { return p ? tag + " y=" + p.y : tag + " nil"; }

console.log(f({ x: 1, y: 2 }), f(null), g({ x: 3, y: 4 }, "g"), g(null, "g"));

const fa: any = f;
const ga: any = g;
console.log(fa([5, 6]), fa("str"), fa(3), fa(null), fa(undefined), fa());
console.log(ga([5, 6], "a"), ga("str", "b"), ga(3, "c"), ga(null, "d"), ga(new Proxy({ x: 7, y: 8 }, {}), "e"));

const box: any = { f, g };
console.log(box.f({ x: 9, y: 10 }), box.f([11, 12]), box.g({ x: 13, y: 14 }, "h"), box.g("str", "i"));
