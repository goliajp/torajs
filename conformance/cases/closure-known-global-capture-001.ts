// free_vars' is_global_name table drifted from the checker's ident
// fallback (rotation 260): a closure body referencing `Iterator`,
// `WeakRef`, or a compiler-synthesized `__torajs_*` helper (Date
// desugar) collected the name as a capture, and the rename broke
// resolution ("closure `__closure_N` references unknown identifier
// `Iterator`" / "`__torajs_date_from_ms`").
function outer() {
  const th = function () {
    return Iterator.from([1, 2]);
  };
  for (const v of th()) console.log(v);
  const mk = function () {
    return new Date(86400000).getTime();
  };
  console.log(mk());
  const wr = function () {
    const target = { a: 1 };
    const r = new WeakRef(target);
    const v: any = r.deref();
    return v.a;
  };
  console.log(wr());
}
outer();
