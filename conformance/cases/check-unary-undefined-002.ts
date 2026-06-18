// ES §13.5.5 (`-x`) / §13.5.6 (`~x`) / §13.5.7 (`!x`) — when x is
// `undefined`: ToNumber(undefined) = NaN per §7.1.4, so `-undefined`
// = NaN. `~undefined` = ToInt32(NaN) ^ -1 = -1 per §7.1.6. `!undefined`
// = true since ToBoolean(undefined) = false. Prior to S136 the Neg /
// BitNot arms rejected Undefined at typecheck; `!undefined` already
// worked. (`+undefined` was fixed in S135.)
console.log(-undefined);
console.log(Number.isNaN(-undefined));
console.log(~undefined);
console.log(!undefined);
// Sanity: existing coercibles still work.
console.log(-null);          // -0 (prints "-0" in bun/v8)
console.log(~null);          // -1
console.log(!null);          // true
console.log(-'10');          // -10 — string ToNumber already works
