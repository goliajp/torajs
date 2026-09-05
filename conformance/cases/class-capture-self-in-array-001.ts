// A class that captures an enclosing local goes down the ES5 lane,
// where its constructor is a function expression that needs receiver
// promotion. Handing its own binding out in an ARRAY LITERAL used to
// cost it that promotion — `() => C`, `() => [C]` and `id(C)` all
// kept it, `() => [C, a]` did not — because the element shape only
// admitted an array literal initializing an exactly-`any` binding.
// The typed lanes have honoured FLAG_CLOSURE_RECV_FIRST since 398-06,
// so the element read shifts argv whichever type the array carries.
function outer() {
  let a = 1;
  class C {
    n = a;
    pair = () => [C, a];
    self = () => C;
  }
  const c = new C();
  console.log(c.n, c.pair()[1], c.pair()[0] === C, c.self() === C);

  // constructing out of the element the array handed back still gives
  // the constructor its own receiver
  const again: any = new (c.pair()[0] as any)();
  console.log(again.n);
}
outer();
