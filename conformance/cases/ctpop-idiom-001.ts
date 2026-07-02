function popcount(x: number): number {
  let n: number = x;
  let count: number = 0;
  while (n !== 0) {
    n = n & (n - 1);
    count = count + 1;
  }
  return count;
}

function popcountResidue(x: number): number {
  let n: number = x;
  let count: number = 0;
  while (n !== 0) {
    n = n & (n - 1);
    count = count + 1;
  }
  return n + count;
}

let total: number = 0;
let i: number = 0;
while (i < 1000) {
  total = total + popcount(i);
  i = i + 1;
}
console.log(total);
console.log(popcount(0));
console.log(popcount(1));
console.log(popcount(255));
console.log(popcount(1023));
console.log(popcount(2147483647));
console.log(popcount(-1));
console.log(popcount(-2147483648));
let residue: number = 0;
let j: number = 0;
while (j < 100) {
  residue = residue + popcountResidue(j);
  j = j + 1;
}
console.log(residue);
