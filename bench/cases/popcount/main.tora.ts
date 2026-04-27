function popcount(x: number): number {
  let n: number = x;
  let count: number = 0;
  while (n !== 0) {
    n = n & (n - 1);
    count = count + 1;
  }
  return count;
}

let total: number = 0;
let i: number = 0;
while (i < 10000000) {
  total = total + popcount(i);
  i = i + 1;
}
console.log(total);
