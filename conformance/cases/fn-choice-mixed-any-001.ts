// a choice mixing a named function with a non-function value joins in
// the any lane, where the function is its own fn object
function inc(n: number): number { return n + 1; }
function h(n: number): number { return n - 1; }
function pick(flip: boolean): (n: number) => number {
  const m = (flip && inc) || h;
  return m;
}
console.log(pick(true)(5), pick(false)(5));

function twice(n: number): number { return n * 2; }
const maybe = Math.random() > 2 ? 0 : twice;
console.log(typeof maybe, maybe === twice);
const k: any = maybe;
console.log(k(21));

let none: ((n: number) => number) | null = null;
const chosen = none ?? inc;
console.log(chosen(1));
const flag = 0;
const orFn = flag || twice;
console.log(typeof orFn, (orFn as any)(4), orFn === twice);
