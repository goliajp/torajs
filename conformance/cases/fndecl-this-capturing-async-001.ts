// rotation 346 — a capturing FnDecl-this inside an argument-position
// async fn-expr: the capturing lane rewrites the decl into a `const`
// that lands inside desugar_async's Try, where the promote candidate
// walk could not re-find it before the nested-list spine (the
// collector skipped compound statement bodies).
function runTest(fn: any): void {
  fn();
}
runTest(async function () {
  const tags: any[] = [];
  function Rec() {
    tags.push("ctor");
    this.n = tags.length;
  }
  const a: any = new Rec();
  const b: any = new Rec();
  console.log(a.n, b.n, tags.length);
  console.log(a instanceof Rec, Object.getPrototypeOf(b) === Rec.prototype);
});
