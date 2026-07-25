// A closure that names the binding it initializes. ES §9.1 says a
// closure captures the BINDING, not a snapshot of its value, so the
// name resolves to whatever the binding holds when the body runs — by
// which time the mint has finished. tr resolved captures against the
// outer scope at the moment the initializer was typed, where the
// binding does not exist yet, and answered "references unknown
// identifier".
//
// The second half is the surface that already worked and must keep
// working: nested `function` declarations recurse and co-recurse, and
// an ordinary non-recursive capture is untouched.

function selfRecursiveFnExpr(): void {
  const fact = function (n: number): number {
    return n <= 1 ? 1 : n * fact(n - 1);
  };
  console.log(fact(5));
}

function selfRecursiveArrow(): void {
  const sum = (n: number): number => (n <= 0 ? 0 : n + sum(n - 1));
  console.log(sum(4));
}

// the binding outlives the frame that declared it: the box has to stay
// alive for the escaped closure to keep finding itself
function escapes(): (n: number) => number {
  const fib = function (n: number): number {
    return n <= 1 ? n : fib(n - 1) + fib(n - 2);
  };
  return fib;
}

// self-reference alongside an ordinary capture — one env carries both
function withCapture(base: number): number {
  const down = (n: number): number => (n <= 0 ? base : down(n - 1) + 1);
  return down(4);
}

// each evaluation is its own closure over its own binding
function independentInstances(limit: number): (n: number) => number {
  const step = (n: number): number => (n >= limit ? n : step(n + 1));
  return step;
}

// already worked — nested declarations see themselves and each other
function nestedDeclarations(): void {
  function fact(n: number): number {
    return n <= 1 ? 1 : n * fact(n - 1);
  }
  function isEven(n: number): boolean {
    return n === 0 ? true : isOdd(n - 1);
  }
  function isOdd(n: number): boolean {
    return n === 0 ? false : isEven(n - 1);
  }
  console.log(fact(6), isEven(10), isOdd(7));
}

// already worked — a capture with no self-reference in sight
function plainCapture(): void {
  const k = 10;
  const addK = (n: number): number => n + k;
  console.log(addK(5));
}

function main(): void {
  selfRecursiveFnExpr();
  selfRecursiveArrow();

  const fib = escapes();
  console.log(fib(10), fib(11));

  console.log(withCapture(100));
  console.log(independentInstances(3)(0), independentInstances(7)(0));

  nestedDeclarations();
  plainCapture();
}

main();

// top-level, both forms, outside any function scope
const topFact = function (n: number): number {
  return n <= 1 ? 1 : n * topFact(n - 1);
};
console.log(topFact(5));

let topSum = (n: number): number => (n <= 0 ? 0 : n + topSum(n - 1));
console.log(topSum(4));
