// S2.35b knife 3 — undefined-valued fields join the data-only
// literal promote (the dynobj init lane stores ANY_UNDEF for the
// `undefined` spelling, so `{u: undefined}` keeps undefined
// identity — not collapsed to null).

var cfg = { a: undefined, b: 1 };
function f() {
  console.log("undef", (cfg as any).a === undefined, (cfg as any).a === null);
  console.log("data", (cfg as any).b);
}
f();

// nested undefined
var deep = { inner: { u: undefined }, tag: null };
function g() {
  console.log("deep", (deep as any).inner.u === undefined, (deep as any).tag === null);
}
g();
