// Lifting a non-capturing nested `function` renames it to
// `__nested_<parent>_<name>_N`, and the rename has to reach the
// arrows that call it. The ident rewrite skipped arrow bodies as
// "separate scopes" — true, but a nested scope SEES its enclosing
// bindings, so the call site was left naming a declaration that no
// longer exists ("unknown identifier `step`", then a ReferenceError
// at run time). `lift_arrow_fns` had already moved the arrow to top
// level by the time the pass ran, so nothing downstream could fix it
// either.
//
// The capturing variant took a different lane and always worked,
// which is why this stayed hidden.
function drive(): void {
  function step(v: any): void {
    console.log("step", v);
  }
  const cb: any = (x: any) => {
    step(x);
    return 0;
  };
  cb(1);
}

// An arrow that rebinds the name keeps its own — the rename must not
// reach through a shadowing parameter.
function shadowed(): void {
  function step(v: any): void {
    console.log("outer", v);
  }
  const cb: any = (step: any) => {
    step(2);
    return 0;
  };
  cb((n: any) => {
    console.log("inner", n);
    return 0;
  });
  step(3);
}

drive();
shadowed();
