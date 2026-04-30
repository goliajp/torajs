function ack(m: number, n: number): number {
  if (m === 0) { return n + 1; }
  if (n === 0) { return ack(m - 1, 1); }
  return ack(m - 1, ack(m, n - 1));
}
console.log(ack(2, 3));
console.log(ack(3, 3));
