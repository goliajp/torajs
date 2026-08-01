// S2.35b knife 2 — a top-level data-only literal object (null /
// nested-literal / array-literal fields, the shapes __inlobj
// refuses) promotes as an Any global so named-fn bodies can read
// it. Method-carrying literals stay main-local (tb2 guard).

// null field
var withNull = { a: null };
// nested literal
var nested = { p: { q: 1 }, tag: "n" };
// array-literal field
var withArr = { xs: [1, 2, 3] };
function f() {
  console.log("null", (withNull as any).a === null);
  console.log("nested", (nested as any).p.q, (nested as any).tag);
  console.log("arr", (withArr as any).xs.length, (withArr as any).xs[2]);
}
f();

// deep nesting mixing all three
var deep = { list: [{ v: null }, { v: 5 }], meta: { name: "d", flags: [true, false] } };
function g() {
  console.log("deep", (deep as any).list[1].v, (deep as any).meta.flags[1]);
}
g();

// expando write from a fn lands on the promoted dynobj
function put() {
  (withNull as any).extra = 11;
}
put();
console.log("expando", (withNull as any).extra);

// main-side reads keep working after promotion
console.log("main", (nested as any).p.q + (withArr as any).xs[0]);
