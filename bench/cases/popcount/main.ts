function popcount(x: number): number {
  let n = x;
  let count = 0;
  while (n !== 0) {
    n = n & (n - 1);
    count++;
  }
  return count;
}

const limit = parseInt(process.argv[2], 10);
let total = 0;
for (let i = 0; i < limit; i++) {
  total += popcount(i);
}
console.log(total);
