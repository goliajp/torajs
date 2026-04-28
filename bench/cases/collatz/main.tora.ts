function steps(n: number): number {
  let count: number = 0;
  while (n !== 1) {
    if ((n & 1) === 0) {
      n = n >> 1;
    } else {
      n = 3 * n + 1;
    }
    count = count + 1;
  }
  return count;
}

let max: number = 0;
let i: number = 1;
while (i <= 1000000) {
  let s: number = steps(i);
  if (s > max) {
    max = s;
  }
  i = i + 1;
}
console.log(max);
