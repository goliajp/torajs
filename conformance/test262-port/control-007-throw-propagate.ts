// Adapted from test262: throw inside a callee propagates through 2+ frames
// without an interim catch.
function leaf(): number { throw 42; }
function mid(): number { return leaf() + 1; }
function top(): number { return mid() + 1; }

function check(): number {
  let caught: number = 0;
  try {
    top();
  } catch (e: number) {
    caught = e;
  }
  if (caught !== 42) { throw "#1"; }
  return 0;
}
console.log(check());
