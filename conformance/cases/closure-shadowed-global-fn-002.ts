// RFC 20260828 knife 3 — the nested-declaration half of 001. A nested
// plain `function` resolves its free names through the same pre-bind of
// top-level fn names, so a param or a `let` that shadows one was read
// as the declaration: `typeof g` answered `function` and printing it
// gave `[Function: g]` where the local held a number.
function g(): number {
  return 99;
}

function viaParam(g: any): void {
  function inner(): void {
    console.log("param", typeof g, g);
  }
  inner();
}
viaParam(5);

function viaLet(): void {
  const g: any = 7;
  function inner(): void {
    console.log("let", typeof g, g);
  }
  inner();
}
viaLet();

// a nested declaration two levels down
function viaDeep(g: any): void {
  function outer(): void {
    function inner(): void {
      console.log("deep", typeof g, g);
    }
    inner();
  }
  outer();
}
viaDeep(11);

// negative — a nested declaration calling a genuine, unshadowed
// top-level function still resolves it without capturing
function top(): number {
  return 42;
}
function viaTop(): void {
  function inner(): void {
    console.log("top", top());
  }
  inner();
}
viaTop();
