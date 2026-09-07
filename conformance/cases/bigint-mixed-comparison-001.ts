// §7.2.13 IsLessThan is defined ACROSS the numeric types: steps
// 3.a-3.b compare a BigInt with a String through StringToBigInt, and
// steps 4-5 compare a BigInt with a Number by mathematical value.
// Only the ARITHMETIC operators refuse a mixed pair, and reading that
// refusal onto comparison rejected `1n < 2` at compile time while
// `(1n as any) < 2` answered true off the very same kernel.
console.log(1n < 2, 2 > 1n, 1n <= 1, 1n >= 1);
console.log(1n < NaN, NaN < 1n);
console.log(9007199254740993n > 9007199254740992);
console.log(-1n < 0, 0 < -1n);
console.log(1n < Infinity, Infinity < 1n, 1n < -Infinity);
console.log(1n < 1.5, 1.5 < 2n);

// The String side goes through StringToBigInt, so a non-numeric
// string makes every comparison false (§7.2.13 step 3.b).
console.log(1n < "2", "2" < 1n, 1n < "abc", "abc" < 1n);
console.log(1n < " 2 ");

let big = 3n;
let small = 2;
console.log(big > small, small < big);
