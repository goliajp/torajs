// 567-05 — a callee that MAY be undefined is still called through
// its signature. `let h: Fn | undefined; h = f; h(3)` is a program
// bun runs, and tr refused to compile it: `not callable: type
// Nullable(Function([Number], Number))`. §13.3.6.2 makes the
// undefined case a runtime TypeError at step 6, after the arguments
// evaluate — the same reason a statically uncallable value
// (`true()`) types its call rather than stopping the compile.
//
// The difference is that this callee IS callable on the arm that
// matters, so it keeps its signature: arity and argument types are
// still checked against it.

let h: ((n: number) => number) | undefined;
h = (n) => n * 2;
console.log(h(3));

// The bare-declaration spelling means the same thing (567-04) and
// reaches the same place.
let g: (n: number) => number;
g = (n) => n + 1;
console.log(g(3));

// Two arguments, a capture, and a named function assigned in.
let two: ((a: number, b: number) => number) | undefined;
two = (a, b) => a * 100 + b;
console.log(two(3, 7));

const cap = 10;
let c: ((n: number) => number) | undefined;
c = (n) => n + cap;
console.log(c(3));

function nm(n: number): number {
  return n + 1;
}
let byname: ((n: number) => number) | undefined;
byname = nm;
console.log(byname(3));

// Reassigned through a branch.
let pick: ((n: number) => number) | undefined;
if (cap > 5) {
  pick = (n) => n - 1;
} else {
  pick = (n) => n - 2;
}
console.log(pick(9));

// Calling one that was never assigned is a TypeError, thrown where
// the call is rather than refused at compile time.
let never: ((n: number) => number) | undefined;
let caught = "none";
try {
  never(1);
} catch (e: any) {
  caught = e instanceof TypeError ? "TypeError" : "other";
}
console.log(caught);
