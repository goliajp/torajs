// A binding declared with a function type contextually types the arrow
// (or function expression) that initializes it — TS infers the params
// from the target type when the arrow leaves them unannotated.
//
// tr seeded closure params from call-argument positions only. A binding
// annotation was never a seed, so an unannotated param kept its `any`
// default while the call site dispatched through the ANNOTATION's
// signature; the two disagreed about what a numeric or boolean argument
// looks like in a register and `const g: (n: number) => number = (n) =>
// n; g(4)` was a two-line SIGSEGV. It only showed up when the body
// actually read the param — an ignored one is never decoded.

type UnaryNumber = (n: number) => number;
type Predicate = (n: number) => boolean;

function main(): void {
  // the shape that crashed: annotated binding, unannotated param, used
  const identity: (n: number) => number = (n) => n;
  console.log(identity(4));

  const inc: (n: number) => number = (n) => n + 1;
  console.log(inc(41));

  // same through a bare type alias — the annotation is the alias NAME,
  // which has to be chased to the function type it stands for
  const double: UnaryNumber = (n) => n * 2;
  console.log(double(21));

  const isBig: Predicate = (n) => n > 10;
  console.log(isBig(11), isBig(2));

  // booleans and strings take the same route
  const negate: (b: boolean) => boolean = (b) => !b;
  console.log(negate(true), negate(false));

  const shout: (s: string) => string = (s) => s.toUpperCase();
  console.log(shout("ok"));

  // several params, and a function expression rather than an arrow
  const add: (a: number, b: number) => number = (a, b) => a + b;
  console.log(add(2, 3));

  const mul: (a: number, b: number) => number = function (a, b) {
    return a * b;
  };
  console.log(mul(6, 7));

  // an explicitly annotated param keeps its own annotation
  const half: (n: number) => number = (n: number): number => n / 2;
  console.log(half(9));

  // fewer params than the type declares is legal, and the extra
  // annotation must not be strapped onto something that isn't there
  const ignores: (n: number) => number = () => 42;
  console.log(ignores(1));

  // a void-returning target must not be promoted to value-returning
  const report: (n: number) => void = (n) => {
    console.log("got", n);
  };
  report(7);

  // contextual typing alongside a capture
  const offset = 100;
  const shifted: (n: number) => number = (n) => n + offset;
  console.log(shifted(5));

  // and alongside the self-reference of closure-self-reference-001
  const fact: (n: number) => number = (n) => (n <= 1 ? 1 : n * fact(n - 1));
  console.log(fact(5));
}

main();

// top level takes the same path
const topInc: (n: number) => number = (n) => n + 1;
console.log(topInc(1));

let topAlias: UnaryNumber = (n) => n - 1;
console.log(topAlias(1));
