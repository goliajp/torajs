function loopSum(n: number, k: number): number {
  let xs: number[] = [];
  for (let i: number = 0; i < n; i = i + 1) {
    xs.push(i);
  }
  // capturing-closure map: ys[i] = xs[i] + k
  let ys: number[] = xs.map((x: number): number => x + k);
  let sum: number = 0;
  for (let i: number = 0; i < ys.length; i = i + 1) {
    sum = sum + ys[i];
  }
  return sum;
}

console.log(loopSum(10000000, 2));
