// Rotation 326 — `return fib` where fib is an escape-boxed binding in
// its owning frame: the ident read answers the box's payload as a
// borrow (the box holds the payload's only stake), the frame exit
// still drops the whole box, and the fn-return contract hands the
// caller an owned value — zero incs, two decs on the closure env.
// A self-referential escaping closure is the minimal shape (the
// self-capture is what forces the binding into a box).
function escapes(): (n: number) => number {
  const fib = function (n: number): number {
    return n <= 1 ? n : fib(n - 1) + fib(n - 2);
  };
  return fib;
}
const fib = escapes();
console.log(fib(10), fib(11));

// each evaluation mints its own box and its own env
function independent(limit: number): (n: number) => number {
  const step = (n: number): number => (n >= limit ? n : step(n + 1));
  return step;
}
console.log(independent(3)(0), independent(7)(0));
console.log("done");
