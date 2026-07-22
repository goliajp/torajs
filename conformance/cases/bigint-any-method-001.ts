// BigInt methods on an any-typed receiver route through the any-lane
// cell dispatch (§21.2.3): toString (radix-aware), toLocaleString
// (grouped decimal), valueOf (thisBigIntValue identity).
const b: any = 1234567n;
console.log(b.toString());
console.log(b.toString(16));
console.log(b.toString(2));
console.log(b.toLocaleString());
console.log(b.valueOf() === 1234567n, typeof b.valueOf());

const z: any = 0n;
console.log(z.toString(), z.toLocaleString());

const neg: any = -98765n;
console.log(neg.toString(), neg.toLocaleString(), neg.toString(16));

// explicit-undefined radix folds to base 10
const u: any = 255n;
console.log(u.toString(undefined));

// out-of-range radix -> catchable RangeError
try {
  console.log((42n as any).toString(1));
} catch (e: any) {
  console.log("caught:", e instanceof RangeError);
}
