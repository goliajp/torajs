// S142 — String + Undefined per ES §13.15.3
// (StringOrNumericBinaryOperator → StringConcat branch when either
// operand is String; ToPrimitive(Default) → ToString on the other
// side gives "undefined" per §7.1.17). Mirrors the String + Null /
// String + Bool arm shipped at V3-18 m1.d.

// both directions for plain string + undefined literal
console.log("1:", "" + undefined);
console.log("2:", undefined + "");
console.log("3:", "head=" + undefined);
console.log("4:", undefined + "=tail");

// substr (sliced view) + undefined — routes through the same str-or-substr arm
const s = "abcdef".slice(1, 3);
console.log("5:", s + undefined);
console.log("6:", undefined + s);

// in template-style concatenation (manual + chain)
console.log("7:", "(" + undefined + ")");
console.log("8:", "a=" + undefined + ",b=" + undefined);
