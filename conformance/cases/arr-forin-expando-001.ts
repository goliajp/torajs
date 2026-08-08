// RFC 20260808 knife 5 — the typed-Arr for-in surface rides the
// same anyv_forin_keys kernel the any tier uses (enumerable-filtered
// index walk + enumerable expando tail), instead of the len-driven
// index-only mint that skipped both expandos and hole exclusion.
var b = [10, 20];
b.p = 9;
for (var k in b) {
  console.log(k);
}

// holes are not enumerated
var c = [1, , 3];
for (var k2 in c) {
  console.log(k2);
}

// non-enumerable expando stays out of for-in
Object.defineProperty(b, "q", { value: 1, enumerable: false });
for (var k3 in b) {
  console.log(k3);
}
