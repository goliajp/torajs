// RFC 20260730-iterator-global 刀 2a — Iterator.prototype.map:
// lazy semantics (no stepping at construction), (value, counter)
// callback args, for-of consumption, chaining, return() close, and
// a generator / builtin-iterator-cell underlying (any lane).

function* g() {
  yield "a";
  yield "b";
  yield "c";
}

// Lazy: mapper must not run until iteration.
let calls = 0;
const it: any = g();
const mapped: any = it.map((v: any, i: any) => {
  calls++;
  return v + i;
});
console.log(calls);
for (const x of mapped) {
  console.log(x);
}
console.log(calls);

// Exhausted helper stays done.
console.log(JSON.stringify(mapped.next()));

// Chaining: map over map (stepwise bindings — the single-expression
// chain `it.map(f).map(g)` trips a pre-existing lowering bug where
// the intermediate temp is over-released, L3b-registered with the
// RFC; the helper-over-helper runtime path is what this exercises).
const it2: any = g();
const inner: any = it2.map((v: any) => v + "!");
const chained: any = inner.map((v: any) => "<" + v + ">");
for (const x of chained) {
  console.log(x);
}

// Builtin iterator cell underlying.
const av: any = [10, 20, 30].values();
const doubled: any = av.map((v: any) => v * 2);
for (const x of doubled) {
  console.log(x);
}

// Map-collection iterator underlying.
const m = new Map();
m.set("k", 7);
const mv: any = m.values();
const plus: any = mv.map((v: any) => v + 1);
for (const x of plus) {
  console.log(x);
}

// return() closes: mapper never runs again after an early break.
let seen = 0;
const it3: any = g();
const m3: any = it3.map((v: any) => {
  seen++;
  return v;
});
for (const x of m3) {
  break;
}
console.log(seen);
console.log(JSON.stringify(m3.next()));

// instanceof: a helper is an Iterator.
const it4: any = g();
console.log(it4.map((v: any) => v) instanceof Iterator);

// Non-callable mapper throws TypeError.
try {
  const it5: any = g();
  it5.map(123);
  console.log("no-throw");
} catch (e) {
  console.log(e instanceof TypeError);
}
