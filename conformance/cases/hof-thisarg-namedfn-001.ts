// §23.1.3.{6,8,12,21,30} — thisArg for a NAMED fn callback over a
// `var arr = […]` receiver: the mutable-but-never-rewritten binding
// carries the same certainty a `const` does, so the knife-4 promote
// admits it and the callback's `this` is the thisArg T.
var res = [];
function cb(v) {
  this.res.push(v * 2);
  return true;
}
var o = { res: res };
var xs = [1, 2, 3];
console.log(xs.every(cb, o));
console.log(res);

// I64 layout slot written through the any lane: the typed lanes box
// a number[] element as F64, and the integer-valued payload converts
// into the I64 slot instead of rejecting (struct_data_field_set).
var acc = { sum: 0 };
function addUp(v) { this.sum += v; }
var ys = [10, 20];
ys.forEach(addUp, acc);
console.log(acc.sum);

// map with a thisArg reading a field.
var env = { k: 100 };
function plusK(v) { return v + this.k; }
console.log([1, 2].map(plusK, env));

// filter + some over the same promoted receiver shape.
var keep = { min: 2 };
function big(v) { return v >= this.min; }
console.log(xs.filter(big, keep));
console.log(xs.some(big, { min: 3 }));
