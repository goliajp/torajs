// §21.1.1.1 step 3 — Number(bigint) is the one legal BigInt→Number
// conversion; the implicit ToNumber lane keeps throwing (§7.1.4).
console.log(Number(10n));
console.log(Number(-3n));
console.log(Number(0n));
console.log(Number(2n ** 64n));
console.log(Number(123456789012345678901234567890n));
console.log(Number(-(2n ** 70n)));
const b: any = 5n;
console.log(Number(b));
const big: any = 2n ** 130n;
console.log(Number(big));
