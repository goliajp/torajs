// A `const` / `let` without an annotation whose initializer holds a
// closure that reads the binding itself. The closure runs after the
// declaration, so the binding is initialized by then (§14.3.1).
const m = new Map<string, any>([["k", () => m.size]]);
console.log(m.get("k")());

const o = { n: 3, f: () => o.n };
console.log(o.f());

let l = { g: () => l };
console.log(l.g() === l);

const w = new WeakRef({ f: () => w });
console.log(typeof w.deref());

const arr = [1, 2, () => arr.length];
console.log((arr[2] as any)());

function scope() {
  const inner = { v: 7, get: () => inner.v };
  return inner.get();
}
console.log(scope());

const nested = { a: { b: () => nested.a } };
console.log(nested.a.b() === nested.a);

const fr = new FinalizationRegistry((h: any) => {
  console.log(typeof fr, h);
});
console.log(typeof fr);
