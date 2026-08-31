// console.log of a BigInt inside a function body: the shared print
// target table had no BigInt arm, so it fell into the print_i64
// catch-all and emitted the raw cell pointer as a decimal, while the
// same statement at top level printed `2n` (lower_top_stmt carries
// its own BigInt arm). Covers the local, the parameter, the arithmetic
// result and console.error, plus the top-level form the two lanes
// must agree on.
const g: bigint = 7n;

function fromLocal(): void {
  const b: bigint = 2n;
  console.log(b);
}

function fromParam(x: bigint): void {
  console.log(x);
}

function fromArith(): void {
  console.log(2n * 3n);
  console.log(6n & 3n);
  console.log(-7n);
}

function fromOuter(): void {
  console.log(g);
}

function viaError(): void {
  console.error(4n);
}

console.log(2n);
fromLocal();
fromParam(11n);
fromArith();
fromOuter();
viaError();
