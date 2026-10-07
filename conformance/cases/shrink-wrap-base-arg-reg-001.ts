// a recursion's base case computes its result from the second param
// while the first param's register is free to take it
function ack(m: number, n: number): number {
  if (m === 0) return n + 1;
  if (n === 0) return ack(m - 1, 1);
  return ack(m - 1, ack(m, n - 1));
}

function down(k: number, acc: number): number {
  if (k <= 0) return acc * 2 + 1;
  return down(k - 1, acc + k) + k;
}

// the base case reads the first param after the second one's result
function mix(a: number, b: number): number {
  if (a < 1) return b * 3 + a;
  return mix(a - 1, b + a) - a;
}

function half(x: number, y: number): number {
  if (x < 0.5) return y / 2;
  return half(x - 0.75, y + x) * 1;
}

const m = parseInt("2", 10);
const n = parseInt("5", 10);
console.log(ack(m, n));
console.log(ack(3, 3));
console.log(ack(Number("1"), Number("7")));
console.log(down(parseInt("10", 10), 0));
console.log(down(Number("2.5"), -0));
console.log(mix(parseInt("6", 10), 1));
console.log(mix(Number("-0"), 4));
console.log(half(Number("3.25"), 1));
console.log(1 / half(Number("0.25"), -0));
