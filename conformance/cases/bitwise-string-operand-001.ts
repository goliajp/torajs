// §13.12 — a String operand of the bitwise family runs ToNumeric
// (ToNumber for a String): "3" & 1 is 1, not a compile-time refusal.

console.log("3" & 1);
console.log("3" | 0);
console.log("abc" & 1);
console.log("abc" | 0);
console.log("3" << 2);
console.log(1 & "3");
console.log("7" >>> 1);
console.log("2.5" & 3);
console.log("" | 0);
console.log(true & "3");
console.log("3" ^ "5");
console.log(null | "2");
console.log("12" >> 1);
console.log("0x10" & 255);

const s = "6";
console.log(s & 3);
function f() {
  return ("10" | 0) + 1;
}
console.log(f());

// a string view (Substr repr) boxes the same way
const v = "x3";
console.log(v.slice(1) & 3);
console.log(v.substring(1) | 0);

// neighbors stay untouched: concat and string ordering
console.log("3" + 1);
console.log("10" < "9");
