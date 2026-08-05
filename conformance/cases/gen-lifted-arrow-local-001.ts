// RFC 20260805-async-fn-state-machine D0, second half — an arrow held
// in a generator local, and what it captures.
//
// Two defects, both reachable from `const f = (n: number) => n + base`
// inside a `function*`:
//
// A: the lifted field's type. `infer_expr_ann_with` cannot answer an
//    arrow here — its `Expr::Closure` arm reads a signature published
//    under a lifted `__closure_*` name, and this pass runs before
//    `lift_arrow_fns`, so the node is still an `Expr::ArrowFn`. The
//    field fell back to `number`: "field is Number, value is
//    Function([Number], Number)", then "not callable: type Number".
//
// B: the capture. `rewrite_params_to_this` moves a generator's params
//    and locals onto `this`, and it did not descend into arrow bodies
//    — so `base` inside the arrow was left naming a binding that no
//    longer exists ("unknown identifier `base`" → ReferenceError).
//    This half fired with an explicit annotation too, so it predates
//    A. Names the arrow rebinds keep their own meaning.
//
// C: an arrow held across a yield boundary is the reason the local is
//    lifted to a field in the first place — it has to still be callable
//    after the generator resumes.

function* g(base: number): any {
  const explicitRet = (n: number): string => "v" + n;
  yield explicitRet(1);

  const captures = (n: number) => n + base;
  yield captures(10);
  yield "mid";
  yield captures(20);

  const k = 100;
  const capturesLocal = (n: number) => n + k;
  yield capturesLocal(1);

  const sideEffect = (n: number) => {
    console.log("side", n);
  };
  sideEffect(5);
  yield "after-void";

  const shadowsParam = (base: number) => base * 2;
  yield shadowsParam(5);
  yield base;

  const shadowsLocal = (n: number) => {
    const k = 1;
    return n + k;
  };
  yield shadowsLocal(10);
  yield k;

  const annotated: (n: number) => number = (n: number) => n + base;
  yield annotated(3);

  return 0;
}

function drain(gg: any): void {
  let r: any = gg.next();
  while (r.done === false) {
    console.log(r.value);
    r = gg.next(0);
  }
}

drain(g(7));
