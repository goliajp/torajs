// A BigInt literal is a fresh rc=1 heap block, but it was absent from
// the owned-shape predicate, so in every consumer position it had no
// release site while the consumer added its own inc on top — the
// chunk-640 array/object-literal failure, one literal shape later.
// This case guards the OTHER direction: with the literal now
// transferring its stake, a consumer must not release it twice.
function take(b: bigint): bigint {
  return b + 1n;
}

const xs: bigint[] = [];
for (let i: number = 0; i < 200; i++) {
  xs.push(2n);
}
console.log(xs.length, xs[0], xs[199]);

let acc: bigint = 0n;
for (let i: number = 0; i < 200; i++) {
  acc = acc + take(3n);
}
console.log(acc);

const boxed: any = 5n;
console.log(boxed, typeof boxed);

const inObj = { v: 7n };
console.log(inObj.v);

const nested: bigint[] = [1n, 2n, 3n];
console.log(nested, nested[1] * 10n);
