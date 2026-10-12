// A `var` is one binding for the whole function body (§14.3.2): a
// redeclaration in a nested block stores into the same binding, and
// the binding reads undefined until something stores into it.
function* g1() {
  var f = 1;
  {
    var f = 2;
  }
  yield f;
}
for (const v of g1()) console.log(v);

function* g2() {
  var x = 1;
  {
    var x = "s";
  }
  yield x;
}
for (const v of g2()) console.log(v);

function* g3() {
  yield x;
  var x = 1;
  yield x;
}
for (const v of g3()) console.log(v);

function* g4() {
  x = 5;
  var x;
  yield x;
}
for (const v of g4()) console.log(v);

function* g5() {
  {
    let f = 1;
    yield f;
  }
  yield f;
  var f = 2;
  yield f;
}
for (const v of g5()) console.log(v);

function* g6() {
  const h = () => x;
  yield h();
  var x = 3;
  yield h();
  yield f();
  function f() {
    return x + 1;
  }
}
for (const v of g6()) console.log(v);

function* g7(a: number) {
  var a = a + 1;
  yield a;
  for (var i = 0; i < 3; i++) {}
  yield i;
}
for (const v of g7(4)) console.log(v);

async function a1() {
  var f = 1;
  {
    var f = 2;
  }
  await 0;
  console.log("a1", f);
}
a1();
