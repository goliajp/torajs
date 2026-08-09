// rotation 346 — a FnDecl whose body reads `this`, nested inside an
// ARGUMENT-POSITION fn-expr. The enclosing lift used to report the
// nested decl's `__this` as its own free var (function-this never
// rides up, §10.2.1.1) and the checker rejected the whole closure
// with "unknown identifier __this". The nested decl itself promotes
// through the construct-channel use shapes.
function runTest(fn: any): void {
  fn();
}
runTest(function () {
  function F() {
    this.x = 1;
  }
  const o: any = new F();
  console.log(o.x);
  console.log(Object.getPrototypeOf(o) === F.prototype);
});
