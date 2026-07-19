// RFC 20260719-ns-static-value-reify B3c-2 — Symbol.for / keyFor read
// as VALUES. The registry kernels are the typed tier's own
// (`__torajs_symbol_for` / `_key_for`); the arm adds the ToString
// coercion temp (dropped — the kernel SHARES the key, it does not
// adopt it) and maps the unregistered-NULL answer to undefined
// rather than a raw null Str slot.
//
// Faces recorded elsewhere, all pre-existing (probed on the DIRECT
// typed-tier call too, so none of them are value-read regressions):
//   * `Symbol.keyFor(x: any)` — checker rejects (`expected Symbol,
//     got Any`) where TS admits any; L3b.
//   * `sym.toString()` — Symbol-receiver member call unsupported;
//     the value-read surface here never touches it.
//   * direct `Symbol.keyFor(unregistered)` prints `null` vs bun's
//     `undefined` (typed-tier Nullable lowering); the value-read
//     lane answers undefined correctly, covered below.

const f = Symbol.for;
const k = Symbol.keyFor;

// registry identity through the value-read cells
const s1 = f("app.key");
console.log(s1 === Symbol.for("app.key"));
console.log(typeof s1);

// keyFor round-trip + the unregistered → undefined face
console.log(k(s1));
const local = Symbol("loc");
console.log(k(local));

// reflection — name / length / native toString / inline print
console.log(f.name, k.name, f.length, k.length);
console.log(String(f));
console.log(f);

// interning — same cell on every read
console.log(Symbol.for === Symbol.for);

// any-lane call + .call re-dispatch
const viaAny: any = f;
console.log(viaAny("z.w") === Symbol.for("z.w"));
console.log(f.call(null, "c.c") === Symbol.for("c.c"));

// churn — the coercion temp and the owned symbol/key results must
// balance (registry hit incs, our drop decs; the key Str answered
// by keyFor is a fresh owned reference each round)
let n = 0;
for (let i = 0; i < 2000; i++) {
  const s = f("app.key");
  if (k(s) === "app.key") n += 1;
}
console.log(n);
