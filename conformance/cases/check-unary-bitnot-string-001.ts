// ES §13.5.6 — unary `~x` calls ToInt32(ToNumber(x)). Prior to S137
// the BitNot arm rejected String operands at typecheck, though Neg
// already routed through __torajs_str_to_number (the strtod-based
// runtime helper). S137 mirrors that wedge for BitNot.
console.log(~'10');      // ToInt32(10) ^ -1 = -11
console.log(~'10.7');    // truncate → ToInt32(10) ^ -1 = -11
console.log(~'abc');     // NaN → ToInt32(NaN) = 0 → -1
console.log(~'');        // empty → 0 → -1
console.log(~'-5');      // ToInt32(-5) ^ -1 = 4
console.log(~'  7  ');   // trimmed → 7 → -8
