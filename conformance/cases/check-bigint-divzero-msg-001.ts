// BigInt divide-by-zero RangeError message — bun spec literal:
// "0 is an invalid divisor value." (NOT "BigInt divide by zero",
// which was tora's ad-hoc msg literal in the pre-port C runtime).
//
// Covers both `/` and `%`, and verifies the binding behaves as a
// RangeError instance after the Type::Any instanceof wedge close
// from the previous chunk.
//
// `e.constructor.name` deliberately NOT exercised: bun returns
// "RangeError", tora returns `undefined` because Type::Any member
// read (ssa_lower_any_member, RFC `any-class-member-read` from the
// 94th session) only dispatches over declared class fields, not
// the implicit `constructor` prototype-chain slot. That's a
// separate wedge tracked in L3b.

try {
  const a = 5n / 0n;
  console.log("unreached:", a);
} catch (e) {
  console.log(e.message);
  console.log(e instanceof RangeError);
}

try {
  const b = 5n % 0n;
  console.log("unreached:", b);
} catch (e) {
  console.log(e.message);
  console.log(e instanceof RangeError);
}

// Larger / negative numerator — div msg must be the same.
try {
  const c = -9999999999999999999999n / 0n;
  console.log("unreached:", c);
} catch (e) {
  console.log(e.message);
}
