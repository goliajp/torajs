// RFC 20260808-construct-channel B6 刀 3 — the typed lowering's
// array-like struct arm walks the REAL index properties (the
// undefined-filled stub is retired): `Array.from({'0': 41, length:
// 3})` reads each index through the unified walk's tail. A nullish
// source compiles and raises §23.1.2.1 step 5's runtime TypeError.
var list = { '0': 41, '1': 42, '2': 43, length: 3 };
const a = Array.from(list);
console.log(a.length, a[0], a[1], a[2]);
var short = { length: 2 };
const b = Array.from(short);
console.log(b.length, b[0], b[1]);
var noLen = { a: 1 };
const c = Array.from(noLen);
console.log(c.length);
try {
  Array.from(null);
} catch (e) {
  console.log("caught-null");
}
try {
  Array.from(undefined);
} catch (e) {
  console.log("caught-undefined");
}
