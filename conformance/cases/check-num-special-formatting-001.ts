// S167 — Number special-value formatting per ES §21.1.3.{2,3,4,5}.
// `toFixed` / `toExponential` / `toPrecision` / `toLocaleString` must
// follow ES spec for NaN / ±Infinity: `"NaN"` / `"Infinity"` /
// `"-Infinity"`. (Bun's en-US `toLocaleString` substitutes Unicode
// `"∞"` / `"-∞"` per CLDR root, but tora's Str layout currently
// stores a code-unit count not a byte count so multi-byte UTF-8 trips
// the print path — that variant is L3b; this fixture skips it.)

// 1) toString — already canonical (regression guard for the wedge).
console.log(NaN.toString());
console.log(Infinity.toString());
console.log((-Infinity).toString());

// 2) toFixed
console.log(NaN.toFixed(2));
console.log(Infinity.toFixed(2));
console.log((-Infinity).toFixed(2));
console.log(NaN.toFixed());
console.log(Infinity.toFixed(0));

// 3) toExponential
console.log(NaN.toExponential(2));
console.log(Infinity.toExponential(2));
console.log((-Infinity).toExponential(2));
console.log(NaN.toExponential());

// 4) toPrecision
console.log(NaN.toPrecision(3));
console.log(Infinity.toPrecision(6));
console.log((-Infinity).toPrecision(3));

// 5) toLocaleString — "NaN" for NaN (Infinity Unicode form deferred).
console.log(NaN.toLocaleString());

// 6) Finite value regression — non-special values unchanged.
console.log((3.14159).toFixed(2));
console.log((1234.5678).toPrecision(6));
console.log((12345.678).toLocaleString());
console.log((1234567).toLocaleString());
console.log((-1234).toLocaleString());
