// RFC 20260713-generator-fn-value-substrate blade 2 — generator
// function EXPRESSIONS parse for real and hoist to decl-form
// (`hoist_gen_fn_exprs`): the parser marks each `function*(){}`
// ArrowFn in ast.gen_fn_exprs; the pass lifts it into a top-level
// `function* __genexpr_N` and swaps the slot for an Ident, so the
// state-machine desugar / factory / fn-addr machinery all reuse the
// decl path. Pre-blade these were drop-the-body parse stubs that
// silently ran an empty closure (never executed the body, returned
// undefined) — silent-wrong, now gone.

// let-bound generator expression.
const ge = function* () {
  yield 43;
  yield 44;
};
const it = ge();
console.log(it.next().value, it.next().value, it.next().done); // 43 44 true

// IIFE form with a parameter.
console.log((function* (n: number) { yield n * 2; })(21).next().value); // 42

// Named generator expression (self-name accepted).
const named = function* helper() {
  yield "hi";
};
console.log(named().next().value);                 // hi

// var-then-assign binding (the test262 staple) with a default param —
// apply_default_args follows the __genexpr_ alias through the assign.
function thrower(): number { throw new Error("boom"); }
let cc = 0;
var f: any;
f = function* (_ = thrower()) {
  cc = cc + 1;
};
try {
  f();
  console.log("no throw");
} catch (e) {
  console.log("threw at call:", (e as Error).message); // threw at call: boom
}
console.log(cc);                                   // 0

// Destructuring pattern param on an expression-form generator
// (gen_param_destr_prefix carries under the hoisted name).
const gd = function* ({ a, b }: any) {
  yield a + b;
};
console.log(gd({ a: 2, b: 3 }).next().value);      // 5

// Manual iteration drain (for-of over a genexpr factory is a recorded
// RFC residual — parse-time generator_fns elem typing can't see
// post-parse hoisted names).
const seq = function* () {
  yield 1;
  yield 2;
  yield 3;
};
let sum = 0;
const sit = seq();
let step = sit.next();
while (!step.done) {
  sum = sum + step.value;
  step = sit.next();
}
console.log(sum);                                  // 6
