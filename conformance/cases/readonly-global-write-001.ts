// §6.2.5.6 PutValue on the non-writable global value properties
// (§19.1 — NaN / Infinity / undefined): in strict code the write is a
// RUNTIME TypeError raised when the assignment is reached — never a
// compile error, and never reached means never raised.

function t1() {
  NaN = 12;
}
try {
  t1();
  console.log("t1-ok");
} catch (e) {
  console.log("t1-threw", e instanceof TypeError);
}

try {
  Infinity = true;
  console.log("t2-ok");
} catch (e) {
  console.log("t2-threw", e instanceof TypeError);
}

// value position throws the same way
try {
  var r = ((undefined) = 5);
  console.log("t3-ok", r);
} catch (e) {
  console.log("t3-threw", e instanceof TypeError);
}

// a user declaration shadows the global — ordinary assignment
var NaN2 = 1;
NaN2 = 2;
console.log("t4", NaN2);

// an unreached write raises nothing
if (false) {
  NaN = 1;
}
console.log("t5", typeof NaN, typeof Infinity, typeof undefined);
