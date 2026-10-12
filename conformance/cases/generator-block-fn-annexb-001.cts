// Annex B §B.3.3 in a generator / async body: a block function in
// sloppy code also writes a var binding of the function where its text
// sits, so the name reads `undefined` before the block and the function
// after it — unless a parameter already owns the name (§B.3.3.1 step
// 1.a.ii).
//
// bun is not an oracle here: it reads every file as strict code, where
// there is no var binding and `h()` after the block throws. node follows
// the spec, so this checks against a `.expected`.

function* g() {
  yield typeof h;
  { function h() { return 1; } }
  yield typeof h;
  yield h();
}
for (const v of g()) console.log(v);

// capturing: the block binding becomes a value reading the frame
function* k() {
  var x = 4;
  yield 0;
  { function c() { return x; } }
  yield c();
}
for (const v of k()) console.log(v);

function* own(h) {
  { function h() { return 2; } }
  yield typeof h;
}
for (const v of own(9)) console.log(v);

async function asy() {
  var z = 8;
  await 0;
  { function d() { return z; } }
  console.log("async", d());
}
asy();
