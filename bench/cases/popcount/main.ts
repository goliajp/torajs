function popcount(x: number): number {
  let n = x;
  let count = 0;
  while (n !== 0) {
    n = n & (n - 1);
    count++;
  }
  return count;
}

let total = 0;
for (let i = 0; i < 10000000; i++) {
  total += popcount(i);
}
console.log(total);
