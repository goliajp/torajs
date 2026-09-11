// A nested generator declaration that reads an enclosing local. The
// lift that moved every nested `function*` to the top level checked
// its body in top-level scope, so the read died as an unknown
// identifier; a capturing one now takes the generator-expression
// route (call-time snapshot of each captured name), the same lane a
// written `function* () {}` expression has had since rotation 348.
function sync() {
  const xs = [1, 2, 3];
  function* g() {
    yield* xs;
  }
  console.log([...g()].join(","));
}
sync();

function recursive() {
  const step = 2;
  function* down(n) {
    if (n > 0) {
      yield n;
      yield* down(n - step);
    }
  }
  console.log([...down(6)].join(","));
}
recursive();

const inArrow = () => {
  const tag = "t";
  function* g() {
    yield tag + 1;
    yield tag + 2;
  }
  return [...g()].join(",");
};
console.log(inArrow());

// No capture: the lift lane, unchanged — including use before the
// declaration, which only a hoisted declaration allows.
function plain() {
  const early = [...g()].join(",");
  function* g() {
    yield 1;
    yield 2;
  }
  console.log(early);
}
plain();

// A top-level binding is a global, not a capture.
const top = [7, 8];
function readsTop() {
  function* g() {
    yield* top;
  }
  console.log([...g()].join(","));
}
readsTop();

// Async form, last: its await is the only suspension point here, so
// the print order does not depend on how the await interleaves.
async function asyncCase() {
  const expected = [0, 1, 2];
  async function* generateInput() {
    yield* expected;
  }
  const out = await Array.fromAsync(generateInput());
  console.log(out.join(","));
}
asyncCase();
