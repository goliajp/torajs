// A self-recursive function that also calls into a mutually recursive
// pair: inlining its own body two levels must not disturb the pair.
function isEven(n: number): boolean {
  if (n === 0) return true;
  return isOdd(n - 1);
}

function isOdd(n: number): boolean {
  if (n === 0) return false;
  return isEven(n - 1);
}

function count(n: number): number {
  if (n === 0) return 0;
  const here = isEven(n) ? 1 : 0;
  return here + count(n - 1);
}

function hof(n: number): number {
  if (n <= 1) return 1;
  return n % 3 === 0 ? hof(n - 1) * 2 : hof(n - 2) + hof(n - 1);
}

console.log(count(20), count(301));
console.log(isEven(10), isOdd(7), isEven(9));
console.log(hof(1), hof(2), hof(3), hof(15));
