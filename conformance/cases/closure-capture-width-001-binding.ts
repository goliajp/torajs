// A lifted closure reads its captures through the binding they had at
// the construction site, not through whatever slot shares the name.
// Every input is computed at run time (argv length is 2 on both sides)
// so no shape folds to a constant.

const two = process.argv.length;
const half = two / 4;
const offset = two + 1;

// captured parameter, no top-level slot of that name
function loopSum(xs: number[], delta: number): number {
  const add = (x: number): number => x + delta;
  let sum = 0;
  for (let i = 0; i < xs.length; i = i + 1) {
    sum = sum + add(xs[i]);
  }
  return sum;
}

// captured parameter shadowing an integral top-level binding
function shadowSum(n: number, offset: number): number {
  const add = (x: number): number => x + offset;
  let sum = 0;
  for (let i = 0; i < n; i = i + 1) {
    sum = sum + add(i);
  }
  return sum;
}

// two closures capture `k` from different bindings
function intSide(k: number): number {
  const g = (x: number): number => x * 2 + k;
  return g(two);
}
function fracSide(k: number): number {
  const g = (x: number): number => x * 2 + k;
  return g(two);
}

// captured let reassigned to a fraction after the closure is built
function lateFrac(n: number): number {
  let step = 1;
  const add = (x: number): number => x + step;
  step = step + half;
  let sum = 0;
  for (let i = 0; i < n; i = i + 1) {
    sum = sum + add(i);
  }
  return sum;
}

// the closure itself writes a fraction into the captured let
function bump(n: number): number {
  let acc = 0;
  const inc = (): void => {
    acc = acc + half;
  };
  for (let i = 0; i < n; i = i + 1) {
    inc();
  }
  return acc;
}

// a closure built inside another closure
function nested(d: number): number {
  const outer = (y: number): number => {
    const inner = (x: number): number => x + d;
    return inner(y) + inner(y + 1);
  };
  return outer(two);
}

const xs: number[] = [];
for (let i = 0; i < 10; i = i + 1) {
  xs.push(i);
}
console.log(loopSum(xs, half));
console.log(loopSum(xs, two));
console.log(shadowSum(10, half), offset);
console.log(intSide(two), fracSide(half));
console.log(lateFrac(4));
console.log(bump(3));
console.log(nested(half));
