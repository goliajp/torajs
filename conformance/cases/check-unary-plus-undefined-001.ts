// ES §13.5.4 — unary `+x` calls ToNumber(x). Per §7.1.4,
// ToNumber(undefined) = NaN. Prior to S135 the check.rs Plus
// arm rejected `+undefined` as "requires number or coercible
// operand", though `+null` (= 0) was accepted. Aligns the
// Undefined case with bun/V8.
console.log(+undefined);
console.log(Number.isNaN(+undefined));
console.log(+null);          // still 0
console.log(+true);          // still 1
console.log(+false);         // still 0
console.log(+'');            // still 0
console.log(+'42');          // still 42
console.log(+'abc');         // still NaN via str_to_number
