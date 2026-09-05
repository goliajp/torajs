// The object-literal half of `class-capture-self-in-array-001`. A
// class that captures an enclosing local goes down the ES5 lane,
// where its constructor is a function expression needing receiver
// promotion. Handing its own binding out in an OBJECT LITERAL used to
// cost it that promotion, the same way an array element did before
// 589-03.
function outer() {
  let a = 1;
  class C {
    n = a;
    wrap = () => ({ k: C, a });
    self = () => C;
  }
  const c = new C();
  console.log(c.n, c.wrap().a, c.wrap().k === C, c.self() === C);

  // constructing out of the field the literal handed back still gives
  // the constructor its own receiver
  const again: any = new (c.wrap().k as any)();
  console.log(again.n);
}
outer();
