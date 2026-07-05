// RFC 20260705 chunk 555 — a side-effecting receiver whose method
// name sits in the str-dispatch match set but whose type makes every
// sub-dispatch decline (I64 toString) must evaluate exactly once.
// Pre-555 the decline fall-through re-lowered the receiver in the
// number-methods arm: getNum() ran twice (tr count=2 vs bun 1).
let count = 0;
function getNum(): number {
  count = count + 1;
  return 5;
}
console.log(getNum().toString());
console.log(count);

let fcount = 0;
function getF(): number {
  fcount = fcount + 1;
  return 2.5;
}
console.log(getF().toString());
console.log(fcount);

// user-class method whose name collides with the RegExp dispatch set
// (`test`) — the regex arm's type decline must not re-run the
// receiver either.
class Checker {
  n: number;
  constructor(n: number) {
    this.n = n;
  }
  test(): boolean {
    return this.n > 0;
  }
}
let ccount = 0;
function getChecker(): Checker {
  ccount = ccount + 1;
  return new Checker(1);
}
console.log(getChecker().test());
console.log(ccount);
