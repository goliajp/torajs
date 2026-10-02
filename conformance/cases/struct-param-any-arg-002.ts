// §10.5.8 — a Proxy handed to a struct-typed parameter through `any`:
// the property read goes through the proxy (forwarded to the target,
// or answered by the get trap), not through the struct layout.
type P = { x: number; y: number };
function f(p: P) { const q = p; return q.x; }
const g: any = f;
console.log("empty proxy ->", g(new Proxy({}, {})));
console.log("forwarding proxy ->", g(new Proxy({ x: 5, y: 6 }, {})));
console.log("trap proxy ->", g(new Proxy({}, { get: () => 42 })));
console.log("cast spelling ->", (f as any)(new Proxy({ x: 9, y: 0 }, {})));
console.log("typed", f({ x: 1, y: 2 }));
