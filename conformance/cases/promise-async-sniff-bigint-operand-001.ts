// Arithmetic over an operand nothing names (a property of an untyped
// parameter) answers a BigInt for BigInt operands, so neither the
// function nor its promise is a number.

async function diff(x) {
  return x.a - x.b;
}

async function negate(x) {
  return -x.a;
}

function syncDiff(x) {
  return x.a - x.b;
}

diff({ a: 5n, b: 2n }).then((v) => console.log("then sub", typeof v, v));
negate({ a: 5n }).then((v) => console.log("then neg", typeof v, v));
diff({ a: 5, b: 2 }).then((v) => console.log("then sub number", typeof v, v));
console.log("sync sub", typeof syncDiff({ a: 7n, b: 2n }), syncDiff({ a: 7n, b: 2n }));
console.log("sync sub number", syncDiff({ a: 7, b: 2 }));

// beside a number the unnamed operand answers a Number or throws
function scaled(x) {
  return x.a * 3;
}
console.log("scaled", typeof scaled({ a: 2 }), scaled({ a: 2 }));
try {
  scaled({ a: 2n });
} catch (e) {
  console.log("scaled bigint", e instanceof TypeError);
}

async function main() {
  const v = await diff({ a: 9n, b: 1n });
  console.log("await sub", typeof v, v);
  const w = await negate({ a: 4 });
  console.log("await neg number", typeof w, w);
}
main();
