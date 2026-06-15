// W-D fn-param-via-Any wedge — `function f(v: any)` called with
// `f(undefined)` vs `f(null)` previously both passed ANY_NULL=0
// at the call boundary (same family as the S126-1 `as any` cast
// wedge, but at three separate SSA-lower fn-call arg-box sites).
//
// Three sites fixed (all routed `box_to_any` → `box_to_any_from_expr`
// so the helper reads the source expression's `expr_types` and
// preserves ANY_UNDEF=5 vs ANY_NULL=0):
//   1. closure-call P0.5 mirror (env-first sig path)
//   2. FnSig local indirect call (`let f = global_fn; f(undefined)`)
//   3. direct fn-call sig path (`f(undefined)` for `function f(v:any)`)
//
// Pre-fix: `check(undefined)` and `check(null)` both surfaced the
// `null is not an object` throw arm of `__torajs_anyv_get_property_descriptor`.
// Post-fix: each surfaces its spec-correct throw message.
//
// `console.log` inside the Type::Any param body also key off the
// runtime tag — pre-fix `console.log(v)` for `check(undefined)`
// printed `null`; post-fix it prints `undefined`.

function probe(v: any, label: string): void {
  console.log(label + ":print:" + v);
  console.log(label + ":typeof:" + typeof v);
  try {
    Object.getOwnPropertyDescriptor(v, "x");
    console.log(label + ":no-throw");
  } catch (e) {
    const ee = e as { name: string; message: string };
    console.log(label + ":throw:" + ee.name);
  }
}

probe(undefined, "undef");
probe(null, "null");

// Closure-call mirror (P0.5) — `let f = (v: any) => ...; f(undefined)`
// — same boxing site under indirect dispatch.
const closure_probe = (v: any, label: string): void => {
  console.log(label + ":print:" + v);
  console.log(label + ":typeof:" + typeof v);
};
closure_probe(undefined, "closure-undef");
closure_probe(null, "closure-null");
