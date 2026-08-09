// rotation 346 — a zero-field object literal's member read answers
// undefined / the runtime-written value instead of a compile-time
// reject: `{}` declares no surface a typo could miss and grows only
// through runtime writes (§10.1.8.1 [[Get]] absent-property
// semantics). A NON-empty literal keeps the loud typo reject.
var f = function () {
  this.touched = true;
};
var obj = {};
f.apply(obj);
console.log(obj.touched);

var bare = {};
console.log(bare.missing);
console.log(typeof bare.missing);

var viaCall = {};
f.call(viaCall);
console.log(viaCall.touched);
