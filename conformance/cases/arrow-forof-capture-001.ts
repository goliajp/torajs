// for-of inside an ARROW function. Only arrows ask the free-variable
// question — the lift pass computes their captures — and the for-of
// desugar's own loop counter was answering it: `elem_expr` is
// `src[i]`, and the walk visited it before binding `i`, so `i` came
// back as a capture of an identifier no scope could ever hold. Every
// source shape below was a type error inside `() => {}` while the
// identical loop in a named fn ran; sibling closure-forof-capture-001
// covers the other direction (a closure created inside a for-of body).

const arr = [1, 2, 3];

const overArray = () => {
  for (const v of arr) {
    console.log("arr", v);
  }
};

const overLiteral = () => {
  for (const v of [10, 20]) {
    console.log("lit", v);
  }
};

const overString = () => {
  for (const c of "hi") {
    console.log("str", c);
  }
};

function* syncGen(): Generator<number> {
  yield 7;
  yield 8;
}

const overHeldGenerator = () => {
  const it = syncGen();
  for (const v of it) {
    console.log("gen", v);
  }
};

// the loop must still report REAL captures: `total` is declared
// outside the loop and read inside it
const summed = () => {
  let total = 0;
  for (const v of arr) {
    total = total + v;
  }
  return total;
};

overArray();
overLiteral();
overString();
overHeldGenerator();
console.log("sum", summed());

async function* ag(): AsyncGenerator<number> {
  yield 1;
  yield 2;
}

// the `(async () => {})()` idiom — an arrow AND the async iterator
// lane at once
(async () => {
  const held = ag();
  for await (const v of held) {
    console.log("fa", v);
  }
})();
