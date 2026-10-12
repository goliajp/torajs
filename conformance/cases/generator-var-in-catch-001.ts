// A `var` written inside a yield-free try / catch, switch or for-of
// body is still the function's binding (§14.3.2) and keeps its value
// across a later yield.
function* g1() {
  try {
    throw 1;
  } catch (e) {
    var x = e + 1;
  }
  yield 0;
  yield x;
}
for (const v of g1()) console.log(v);

function* g2() {
  try {
    throw 1;
  } catch (e) {
    var x = e + 1;
    var h = () => x;
  }
  yield 0;
  yield h();
}
for (const v of g2()) console.log(v);

function* g3() {
  try {
    throw 1;
  } catch (e) {
    var x = e + 1;
    yield 0;
    yield x;
  }
}
for (const v of g3()) console.log(v);

function* g4() {
  for (const v of [1]) {
    var y = v + 1;
  }
  yield 0;
  yield y;
}
for (const v of g4()) console.log(v);

function* g5() {
  switch (1) {
    case 1:
      var y = 3;
  }
  yield 0;
  yield y;
}
for (const v of g5()) console.log(v);

function* g6() {
  for (const k in { a: 1 }) {
    var y = k;
  }
  yield 0;
  yield y;
}
for (const v of g6()) console.log(v);

async function a1() {
  try {
    throw 1;
  } catch (e) {
    var x = e + 1;
    var h = () => x;
  }
  await 0;
  console.log("a1", x, h());
}
a1();
