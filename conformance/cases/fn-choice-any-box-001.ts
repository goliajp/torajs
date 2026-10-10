// A choice among top-level functions, or a call that returns one,
// handed to an `any` slot boxes as the fn object of the function the
// pointer is: the same object every other read of that name answers.
function sq(n: number): number {
  return n * n;
}
function dbl(n: number): number {
  return n * 2;
}
function pick2(k: number): (n: number) => number {
  return k === 0 ? sq : dbl;
}
function pick3(k: number): (n: number) => number {
  return pick2(k);
}
function apply(f: (k: number) => (n: number) => number, k: number): number {
  return f(k)(5);
}

// reading the HOF and the factories as values makes their returns
// cross into `any`
const ap: any = apply;
const pp: any = pick2;
const p3: any = pick3;
console.log(apply(pick2, 0), apply(pick2, 1), apply(pick3, 0), apply(pick3, 1));
console.log(typeof ap, typeof pp, typeof p3);

const loc = (k: number): ((n: number) => number) => (k === 0 ? sq : dbl);
console.log(apply(loc, 0), apply(loc, 1));

let a: any = pick2(0);
console.log(a(9), a === sq, a === dbl);
a = pick3(1);
console.log(a(9), a === dbl);
const flip = process.argv.length > 99;
a = flip ? sq : dbl;
console.log(a(2), a === dbl);
