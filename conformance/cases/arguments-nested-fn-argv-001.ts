// rotation 362 — fn-nested arguments value reads: the binding-chain
// seeds (direct + alias) and the alias-init exemption walk nested
// bodies, so a closure declared inside a function joins the argv
// face like a top-level one. A name bound more than once anywhere is
// conservatively skipped (the by-name kill walk cannot split scopes).
function outer() {
  const h = function () {
    return arguments[0];
  };
  return h(9); // nested direct lane
}
console.log(outer());

function outer2() {
  const f = function () {
    return arguments[0];
  };
  const box = [f];
  return box[0](42); // nested container-store, element call
}
console.log(outer2());

function outer3() {
  const m = function () {
    return arguments.length + (arguments[0] ?? 0);
  };
  const g = m;
  return g(5) + m(1, 2); // nested alias + direct on the same fn
}
console.log(outer3());

function outer4() {
  const k = function () {
    return arguments.length;
  };
  return k(1, 2, 3); // nested length-only (pre-existing lane held)
}
console.log(outer4());
