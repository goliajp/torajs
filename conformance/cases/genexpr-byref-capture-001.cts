// RFC 20260912 — a generator expression captures an enclosing local BY
// REFERENCE: writes in the body land on the live binding, reads see
// reassignments between resumptions, nested closures share the box,
// return()/throw() run the finally that writes it. Async form last.
// --- d1
function outer1() {
  var callCount = 0;
  var ref = function*() {
    callCount = callCount + 1;
    yield 1;
    callCount = callCount + 10;
  };
  var it = ref();
  it.next();
  var mid = callCount;
  it.next();
  return [mid, callCount];
}
console.log(outer1());
// --- d2
function outer2() {
  let n = 1;
  const g = function*() {
    yield n;
    yield n;
  };
  const it = g();
  const a = it.next().value;
  n = 5;
  const b = it.next().value;
  console.log(a, b, n);
}
outer2();
// --- d3
function outer3() {
  let finallyCounter = 0;
  const input = { v: 1 };
  const createInput = function*() {
    try {
      yield input;
    } finally {
      finallyCounter++;
      (() => { finallyCounter += 100; })();
    }
  };
  const it = createInput();
  it.next();
  it.return();
  console.log(finallyCounter, input.v);
}
outer3();
// --- d5
function outer4() {
  let seen = 0;
  const g = function*() {
    try { yield 1; yield 2; } finally { seen++; }
  };
  const a = g(), b = g();
  a.next(); b.next();
  try { a.throw(new Error("x")); } catch (e) { console.log("caught", e.message); }
  b.return(9);
  console.log(seen);
}
outer4();
// --- d6
function outer5() {
  let o = { a: 1 };
  const g = function*() {
    o = { a: 2 };
    yield o.a;
  };
  console.log(g().next().value, o.a);
}
outer5();
// --- d7
// declaration form nested in a function, captured + written (634-03 residual)
function outer6() {
  let finallyCounter = 0;
  function* createInput() {
    try { yield 1; } finally { finallyCounter++; }
  }
  const it = createInput();
  it.next(); it.return();
  return finallyCounter;
}
console.log(outer6());
// --- d4
async function main7() {
  let flag = 0;
  var g = async function*() {
    flag = flag + 1;
    yield 1;
    flag = flag + 1;
  };
  for await (const v of g()) {}
  console.log("flag", flag);
}
main7();
