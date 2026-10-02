// Class instances handed to a struct-typed parameter through `any`: an
// instance whose layout matches field for field takes the typed body,
// one with the fields in another order reads its own `x`, and one
// without the field answers undefined.
type P = { x: number; y: number };
function f(p: P) { const q = p; return q.x; }
class C { x = 5; y = 6; }
class D { y = 1; x = 7; z = 2; }
class E { y = 1; z = 2; }
const g: any = f;
console.log("same order ->", g(new C()));
console.log("reordered ->", g(new D()));
console.log("missing ->", g(new E()));
console.log("typed", f(new C()));
