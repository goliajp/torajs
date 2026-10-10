// a declared function stored into a binding that holds closures: the
// binding calls whichever function it holds now, and holds the same
// function object every other read of that function answers
function h(x: number): number {
  return x * 2;
}
function inc(x: number): number {
  return x + 1;
}

// program level: function first, then another function
let k = h;
console.log(k(3));
k = inc;
console.log(k(3), k === inc, k === h);

// arrow first, then a function
let m = (x: number) => x - 1;
console.log(m(5));
m = h;
console.log(m(5), m === h);

// function first, then an arrow
let p = h;
p = (x: number) => x * 100;
console.log(p(2));
p = inc;
console.log(p(2));

// a choice among functions
const flip = Number("1") > 0;
m = flip ? inc : h;
console.log(m(5), m === inc);
m = !flip ? inc : h;
console.log(m(5), m === h);
m = inc || h;
console.log(m(5), m === inc);
m = (flip, h);
console.log(m(5));

// inside a function body, with a capturing arrow
function local(n: number): number {
  let f = (x: number) => x + n;
  const before = f(1);
  f = inc;
  const mid = f(1);
  f = n > 2 ? h : inc;
  return before * 100 + mid * 10 + f(1);
}
console.log(local(4), local(1));

function localFirst(n: number): string {
  let f = h;
  const a = f(n);
  f = (x: number) => x - n;
  const b = f(10);
  f = inc;
  return [a, b, f(n)].join(",");
}
console.log(localFirst(3));

// a loop that keeps the old values
const fs: ((x: number) => number)[] = [];
let g = (x: number) => x;
for (let i = 0; i < 4; i++) {
  fs.push(g);
  g = i % 2 === 0 ? h : inc;
}
const outs: number[] = [];
for (const f of fs) {
  outs.push(f(10));
}
console.log(outs.join(","), g(10));
console.log(fs[1] === h, fs[2] === inc, fs[3] === h, g === inc);

// capturing arrows and functions taking turns
let base = 7;
let t = (x: number) => x + base;
const seen: number[] = [];
for (let i = 0; i < 4; i++) {
  seen.push(t(i));
  if (i % 2 === 0) {
    t = h;
  } else {
    base += 10;
    t = (x: number) => x + base;
  }
}
console.log(seen.join(","), t(1));

// a top-level binding a function body reads and writes
let op = h;
function useOp(x: number): number {
  return op(x);
}
function setOp(which: number): void {
  op = which > 0 ? inc : h;
}
console.log(useOp(4));
setOp(1);
console.log(useOp(4), op === inc);
op = (x: number) => -x;
console.log(useOp(4));
setOp(0);
console.log(useOp(4), op === h);
