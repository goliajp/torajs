// A function declared in a block of a generator body reads the
// generator's locals (§14.2.3: the block binding is created on block
// entry, in an environment whose outer is the function's). The machine
// spreads the body over its states, and the declaration used to reach
// top level with nothing to capture: `x is not defined`.

function* m() {
  let x = 5;
  yield 0;
  {
    function h() { return x; }
    yield h();
  }
}
for (const v of m()) console.log(v);

function* shapes(base: number) {
  let x = 5;
  yield 0;
  {
    function h() { return x + base; }
    yield h();
    x = 6;
    yield h();
  }
  for (const k of [1, 2]) {
    function at() { return k * 10 + x; }
    yield at();
  }
  try {
    throw 3;
  } catch (e) {
    function show() { return "caught " + e; }
    yield show();
  }
  if (base > 0) {
    let y = 100;
    function inner() { return y + x; }
    yield inner();
  }
}
for (const v of shapes(1)) console.log(v);
