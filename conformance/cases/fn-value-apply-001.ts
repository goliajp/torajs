// L3b — `Function.prototype.apply` on a statically fn-typed VALUE,
// mirroring fn-value-call-001: thisArg evaluates for effect then
// drops (no-this subset), the LITERAL argsArray elements replay the
// value-callee call. Covers the ns-static cells too — `.apply` on
// them rejected as `not callable: type Any` while `.call` worked,
// and the shipped fixtures only ever exercised `.call`.
//
// One recorded boundary: a runtime (non-literal) argsArray keeps
// the loud reject until a variadic spread substrate exists. The
// DIRECT member form (`Math.max.apply(...)`) works too — an
// ns-static member callee takes the boxed dual entry (its fn_addr
// is the typed-slot boundary throw), any other fn-typed member the
// env-first / direct emitters.
function add(a: number, b: number): number {
  return a + b;
}
const f = add;
console.log(f.apply(undefined, [2, 3]));

const mul = (x: number, y: number): number => x * y;
console.log(mul.apply(null, [4, 5]));

// thisArg evaluates for effect, then drops
function withEffect(): number {
  console.log("effect");
  return 0;
}
console.log(f.apply(withEffect(), [1, 2]));

// fn-typed param inside a HOF
function hof(cb: (n: number) => number): number {
  return cb.apply(undefined, [10]);
}
console.log(hof((n: number) => n + 7));

// ns-static value cells — the face that motivated the wedge
const k = Object.keys;
console.log(k.apply(null, [{ a: 1, b: 2 }]));
const m = Math.max;
console.log(m.apply(null, [7, 2]));
const sf = Symbol.for;
console.log(sf.apply(null, ["ap.key"]) === Symbol.for("ap.key"));

// call/apply parity on the same value
console.log(f.call(null, 8, 9), f.apply(null, [8, 9]));

// direct member forms — ns-static cells ride the boxed dual entry,
// a struct-field closure the env-first ABI
console.log(Math.max.apply(null, [3, 9]), Math.max.call(null, 3, 9));
console.log(Object.keys.apply(null, [{ p: 1, q: 2 }]));
console.log(Symbol.for.call(null, "d.d") === Symbol.for("d.d"));
const o = { g: (a: number, b: number): number => a * b };
console.log(o.g.call(null, 2, 3), o.g.apply(null, [4, 5]));
