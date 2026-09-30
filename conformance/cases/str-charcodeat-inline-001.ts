// ES §22.1.3.2 String.prototype.charCodeAt on an owned string: the
// code unit inside [0, length), NaN outside it, ToIntegerOrInfinity
// on the index. Latin-1 and UTF-16 payloads, a surrogate pair and a
// non-BMP scalar, read at a run-time index.
function unit(s: string, i: number): number {
  return s.charCodeAt(i);
}

const lat = "aZ0 ~éÿ";
const wide = "a中😀z\u{1F600}é";

for (let i = -1; i <= lat.length; i = i + 1) {
  console.log("lat", i, unit(lat, i), lat.charCodeAt(i));
}
for (let i = -1; i <= wide.length; i = i + 1) {
  console.log("wide", i, unit(wide, i), wide.charCodeAt(i));
}

// index = length, negative, fractional, NaN, ±Infinity, -0
console.log(lat.charCodeAt(lat.length), wide.charCodeAt(wide.length));
console.log(lat.charCodeAt(-1), wide.charCodeAt(-3));
console.log(lat.charCodeAt(1.9), lat.charCodeAt(-0.5), wide.charCodeAt(2.999));
console.log(lat.charCodeAt(NaN), wide.charCodeAt(NaN));
console.log(lat.charCodeAt(Infinity), lat.charCodeAt(-Infinity), wide.charCodeAt(Infinity));
console.log(lat.charCodeAt(-0), wide.charCodeAt(-0));

// the value is a Number wherever it is observed
const oob = unit(wide, 99);
console.log(typeof oob, Number.isNaN(oob), oob === oob, oob + 1);
const inb = unit(wide, 1);
console.log(inb === 0x4e2d, inb + 1, inb / 2, inb | 0);

// the empty string is out of range everywhere
console.log("".charCodeAt(0), "".charCodeAt(-1), unit("", 0));

// a string built at run time takes the same read
let built = "";
for (let k = 0; k < 3; k = k + 1) built = built + String.fromCharCode(0x41 + k, 0x3b1 + k);
console.log(built, built.length, built.charCodeAt(0), built.charCodeAt(1), built.charCodeAt(5), built.charCodeAt(6));
