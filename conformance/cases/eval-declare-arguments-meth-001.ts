// 425 刀 2 — §19.2.1.3 EvalDeclarationInstantiation: a direct eval
// in an object-literal METHOD's default-parameter position whose
// source var-declares `arguments` throws a SyntaxError when the
// method is called. A method parses as an arena ArrowFn, but
// `objlit_method_exprs` remembers it binds its own `this` /
// `arguments` — so the param-default ownership walk admits it
// exactly like a function expression (the t262
// meth-*-declare-arguments family, 12 cases).
let o = {
  f(p = eval("var arguments")) {
    return 1;
  },
  ok(q = 5): number {
    return q;
  },
};
try {
  o.f();
  console.log("no throw");
} catch (e: any) {
  console.log("threw:", e instanceof SyntaxError);
}
console.log(o.ok());
