// arguments in generator bodies (RFC 20260801-arguments-method-face
// knife 2a): the body's arguments touches ride a captured class
// field whose value is the factory's [...arguments] - the inline
// spread expands the exact call-site argv when the factory joins
// the static-argv face.

// 1. zero-declared, over-arity (gen-func-decl-args-trailing-comma
//    family shape).
function* gf1() {
  console.log(arguments.length);
  console.log(arguments[0], arguments[1]);
}
gf1(42, 'TC39',).next();

// 2. declared param + over-arity.
function* gf2(a) {
  console.log(arguments.length, a, arguments[1]);
}
gf2(1, 2).next();

// 3. reads across yield resumptions (state machine persistence).
function* gf3(a) {
  yield arguments.length;
  yield arguments[0];
}
var it3 = gf3(9);
console.log(it3.next().value, it3.next().value);

// 4. bare escape inside the body.
function* gf4() {
  var xs = arguments;
  yield xs.length;
  yield xs[1];
}
var it4 = gf4(7, 8);
console.log(it4.next().value, it4.next().value);

// 5. spread of arguments inside the body.
function* gf5() {
  yield [...arguments].join(",");
}
console.log(gf5(4, 5, 6).next().value);
