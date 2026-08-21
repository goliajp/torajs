// ES §22.1.2.1 `ToUint16` / §22.1.2.2 `ToNumber` — both steps coerce,
// and both need the Number itself: ±∞ folds to +0 for fromCharCode,
// and fromCodePoint rejects a non-integral code point.
console.log(String.fromCharCode("65"));
console.log(String.fromCharCode("0") === "\x00");
console.log(String.fromCharCode("6", "5"));
console.log(String.fromCharCode(true), String.fromCharCode(undefined) === "\x00");
console.log(String.fromCharCode(65.7));
console.log(String.fromCharCode(Infinity) === "\x00");
console.log(String.fromCharCode(-Infinity) === "\x00");
console.log(String.fromCharCode(Number.NaN) === "\x00");
console.log(String.fromCharCode(-1) === "￿");
console.log(String.fromCharCode(65536 + 65));

console.log(String.fromCodePoint("65"));
console.log(String.fromCodePoint(0x1f600));
for (const bad of [3.14, "3.14", undefined, "_1", "1a", -1, Infinity, Number.NaN, 1114112]) {
  try {
    String.fromCodePoint(bad as any);
    console.log("NO THROW", bad);
  } catch (e) {
    console.log("RangeError", e instanceof RangeError);
  }
}
try {
  String.fromCodePoint(42, 3.14);
} catch (e) {
  console.log("pair", e instanceof RangeError);
}

// Through the function value, with Number operands.
const f = String.fromCodePoint;
console.log(f(65), f(0x1f600));
try { f(3.14); } catch (e) { console.log("cell", e instanceof RangeError); }
console.log(String.fromCodePoint.call(null, 66, 67));
console.log(String.fromCharCode.apply(null, [68, 69]));
