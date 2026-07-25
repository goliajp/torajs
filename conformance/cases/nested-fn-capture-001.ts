// A nested `function` declaration that reads something from the
// function around it. Every nested declaration used to be lifted to
// top level, and a top-level body is checked in top-level scope, so
// any reference to an outer local answered "unknown identifier".
//
// The capturing ones now become `let f = function (…) {…}` where they
// stood, and pick up an env from the arrow-fn lift like any closure.
// Non-capturing declarations stay on the lifting lane untouched.

// the four shapes that used to fail: a local, a parameter, a write
// back to an outer binding, and a sibling nested declaration
function readsLocal(): void {
  const o = { v: 3 };
  function g(): number {
    return o.v;
  }
  console.log(g());
}

function readsParam(p: number): void {
  function g(): number {
    return p + 1;
  }
  console.log(g());
}

function writesOuter(): void {
  let n = 1;
  function bump(): void {
    n = n + 1;
  }
  bump();
  bump();
  console.log(n);
}

function readsSibling(): void {
  const o = { v: 4 };
  function h(): number {
    return g() + 1;
  }
  function g(): number {
    return o.v;
  }
  console.log(h(), g());
}

// recursion needs no new machinery once rewritten — a self-recursive
// declaration is a closure naming the binding it initializes, and a
// mutual pair is one closure capturing a peer declared later
function recursesWhileCapturing(): void {
  const step = 2;
  function isEven(n: number): boolean {
    if (n === 0) {
      return true;
    }
    return isOdd(n - step);
  }
  function isOdd(n: number): boolean {
    if (n === 0) {
      return false;
    }
    return isEven(n - step);
  }
  function countDown(n: number): number {
    if (n <= 0) {
      return step;
    }
    return 1 + countDown(n - 1);
  }
  console.log(isEven(10), isOdd(10), countDown(4));
}

// a declaration two levels down captures outward on its own: the inner
// one is already a closure when the outer one's free variables are read
function nestedTwoDeep(): void {
  const o = { v: 6 };
  function outer(): number {
    function deep(): number {
      return o.v * 2;
    }
    return deep();
  }
  console.log(outer());
}

// a block is its own statement list and routes on its own
function inBlock(flag: boolean): void {
  const base = 10;
  if (flag) {
    function inner(): number {
      return base + 1;
    }
    console.log(inner());
  }
}

// a capturing declaration beside a non-capturing one: only the first
// changes lanes, the second still lifts
function mixedList(): void {
  const o = { v: 5 };
  function g(): number {
    return o.v;
  }
  function plain(): number {
    return 1;
  }
  console.log(g() + plain());
}

// unchanged ground: no captures at all, so these stay on the lifting
// lane — self-recursion, mutual recursion, and calling before the
// declaration (which only the lifting lane can answer)
function noCaptureRecursion(): void {
  function fact(n: number): number {
    if (n <= 1) {
      return 1;
    }
    return n * fact(n - 1);
  }
  function even(n: number): boolean {
    if (n === 0) {
      return true;
    }
    return odd(n - 1);
  }
  function odd(n: number): boolean {
    if (n === 0) {
      return false;
    }
    return even(n - 1);
  }
  console.log(fact(5), even(10), odd(7));
}

function callsBeforeDeclaration(): void {
  console.log(early());
  function early(): number {
    return 9;
  }
}

function main(): void {
  readsLocal();
  readsParam(41);
  writesOuter();
  readsSibling();
  recursesWhileCapturing();
  nestedTwoDeep();
  inBlock(true);
  mixedList();
  noCaptureRecursion();
  callsBeforeDeclaration();
}

main();
