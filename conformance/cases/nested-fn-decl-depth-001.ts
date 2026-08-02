// desugar_nested_fns runs to a fixpoint (r283): a lifted FnDecl can
// itself contain a nested FnDecl — the t262 async-case tail declares
// `$DONE` inside `assertions` inside the case body (120-case
// cluster). Each round walks top-level decls only, so freshly-lifted
// bodies get their own round.
function outer(): number {
  function assertions(): number {
    function inner2(e: number): number {
      return e + 1;
    }
    return inner2(6);
  }
  return assertions();
}
console.log(outer());
const p: any = Promise.resolve(1)
  .then(function (): any {
    function assertions(): number {
      function inner2(e: number): number {
        return e + 1;
      }
      return inner2(41);
    }
    return assertions();
  })
  .then(function (v: any): void {
    console.log(v);
  });
