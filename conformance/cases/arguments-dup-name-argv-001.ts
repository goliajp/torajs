// rotation 362 — dup-named bindings across distinct fns (the test262
// helper idiom): the group admits per-scope when every instance sits
// in its own top-level fn body, every arena use lands inside exactly
// one owner's mask, and each instance passes the solo legal-use test.
function a1() {
  const f = function () {
    return arguments[0];
  };
  return f(1); // direct lane, instance 1
}
function a2() {
  const f = function () {
    return arguments[0];
  };
  const box = [f];
  return box[0](2); // container-store lane, instance 2, same name
}
console.log(a1() + a2());

function b1() {
  const cb = function () {
    return arguments.length + (arguments[0] ?? 0);
  };
  return cb(1, 2);
}
function b2() {
  const cb = function () {
    return arguments.length + (arguments[1] ?? 0);
  };
  return cb(5, 6);
}
console.log(b1() + b2()); // 3 + 8 — dup value bodies, both argv tier
