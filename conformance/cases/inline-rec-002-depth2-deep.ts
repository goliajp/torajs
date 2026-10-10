// Self-recursion inlined two levels deep: results, side-effect order
// and deep frames must match a plain recursive evaluation.
function ack(m: number, n: number): number {
  if (m === 0) return n + 1;
  if (n === 0) return ack(m - 1, 1);
  return ack(m - 1, ack(m, n - 1));
}

function sumTo(n: number): number {
  if (n === 0) return 0;
  return n + sumTo(n - 1);
}

const order: number[] = [];
function walk(n: number): void {
  if (n <= 0) return;
  order.push(n);
  walk(n - 1);
  order.push(-n);
  walk(n - 2);
}

function build(n: number): string {
  if (n === 0) return "";
  return build(n - 1) + String(n % 10);
}

function fib(n: number): number {
  if (n < 2) return n;
  return fib(n - 1) + fib(n - 2);
}

console.log(ack(2, 3), ack(3, 3), ack(3, 5));
console.log(sumTo(10), sumTo(5000));
walk(5);
console.log(order.join(","));
console.log(build(25));
console.log(fib(0), fib(1), fib(2), fib(3), fib(25));
