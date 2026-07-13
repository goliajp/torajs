// RFC 20260713-loose-eq-substrate blade 2 — static BigInt loose
// equality mixes route through the runtime ladder (exact
// mathematical compare, StringToBigInt grammar).

// bigint × bigint value equality (distinct cells)
const a = 1n;
const b = 1n;
console.log(a == b);
console.log(a != b);
console.log(a == 2n);
console.log(-3n == -3n);

// bigint × number (§7.2.14 step 13 — exact, no f64 rounding)
console.log(0n == 0);
console.log(1n == 1);
console.log(1n == 1.5);
console.log(0n == -0);
console.log(1n == 2);
console.log(9007199254740993n == 9007199254740992);

// bigint × boolean (step coerces boolean to number first)
console.log(0n == false);
console.log(1n == true);
console.log(2n == true);

// bigint × string (StringToBigInt; invalid grammar never equals)
console.log(0n == "");
console.log(1n == "1");
console.log(255n == "0xff");
console.log(1n == "1.0");
console.log(1n == " 1 ");
console.log(-7n == "-7");
console.log(1n == "1n");

// symmetric sides
console.log(1 == 1n);
console.log(true == 1n);
console.log("16" == 16n);
