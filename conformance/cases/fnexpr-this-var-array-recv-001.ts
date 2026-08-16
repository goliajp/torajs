// Receiver certainty used to be proved by proving a `const`'s
// initializer, which is a proof only because a `const` cannot be
// reassigned. test262 writes `var xs = [1, 2, 3]` far more often, and
// by the time the pass looks `desugar_var_hoist` has split that into a
// mutable `let xs: any = <uninit>` plus a statement-position
// `xs = [1, 2, 3]` — the receiver is every bit as certain, but the
// initializer stopped carrying it.
//
// So the bar widened to "the whole program writes this name exactly
// once and binds it nowhere else". §23.1.3.15's `Call(callbackfn,
// thisArg, …)` with no thisArg written still hands the callback
// `undefined`, and a function EXPRESSION in that slot used to refuse
// to compile.
//
// A second write puts the name back on the loud reject, which is why
// that half cannot be a case here — a refusal produces no output to
// compare. Probed by hand instead: `var xs = [3, 1, 2];
// xs.forEach(function () { console.log(this); }); xs = [4];` still
// answers `type error: closure __closure_0 references unknown
// identifier __this`.

var xs: any = [3, 1, 2];

xs.forEach(function () {
  console.log(typeof this, this === undefined);
});

// the comparator slot takes no thisArg at all (§23.1.3.30 step 5's
// `Call(comparator, undefined, «x, y»)`), so any argument count admits
var ss: any = [3, 1, 2];
console.log(
  ss
    .sort(function (a: any, b: any) {
      return this === undefined ? a - b : b - a;
    })
    .join(","),
);

// `let`, written once and never again, is the same proof
let ys: any = [10, 20];
console.log(
  ys
    .map(function (v: any) {
      return v + ":" + typeof this;
    })
    .join("|"),
);

// the value has to be written by the ONE write for the name to carry
// it — here the write is the declaration's own initializer
var zs: any = [5];
console.log(
  zs.reduce(function (acc: any, v: any) {
    return acc + v + (this === undefined ? 0 : 100);
  }, 1),
);
