// RFC 20260713-defprop-residual-cluster chunk A — defineProperties
// returns the receiver; the statement discard's rc dec buffers the
// array as a cycle candidate (Purple paint, header bits 13-14).
// Pre-fix FLAG_ARR_LENGTH_RO shared bit 14, so the paint read back as
// a locked length: gOPD answered writable:false and a later length
// assign threw on a plain array.
var arr = [];
Object.defineProperties(arr, {});
var d = Object.getOwnPropertyDescriptor(arr, "length");
console.log(d.writable);
arr.length = 2;
console.log(arr.length);

var brr = [];
Object.defineProperties(brr, { length: {} });
var d2 = Object.getOwnPropertyDescriptor(brr, "length");
console.log(d2.writable);
brr.length = 3;
console.log(brr.length);

// The real length lock still works after the bit move.
var crr = [];
Object.defineProperty(crr, "length", { writable: false });
var d3 = Object.getOwnPropertyDescriptor(crr, "length");
console.log(d3.writable);
try {
  crr.length = 5;
  console.log("no-throw");
} catch (e) {
  console.log("threw");
}
