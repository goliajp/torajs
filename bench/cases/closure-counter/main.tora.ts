function loopSum(xs: number[], offset: number): number {
  // closure captures `offset` from the enclosing fn — env-block path.
  let add = (x: number): number => x + offset;
  let sum: number = 0;
  for (let i: number = 0; i < xs.length; i = i + 1) {
    sum = sum + add(xs[i]);
  }
  return sum;
}

let xs: number[] = [];
for (let i: number = 0; i < 10000000; i = i + 1) {
  xs.push(i);
}
console.log(loopSum(xs, 2));
