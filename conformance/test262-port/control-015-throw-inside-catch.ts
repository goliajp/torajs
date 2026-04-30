// Adapted from test262: throw inside catch — re-throws to the outer
// frame's catch (or propagates if none).
function inner(): number {
  throw 7;
}

function middle(): number {
  try {
    return inner();
  } catch (e: number) {
    throw e + 100;     // 107
  }
}

function check(): number {
  let caught: number = 0;
  try {
    middle();
  } catch (e: number) {
    caught = e;
  }
  if (caught !== 107) { throw "#1"; }
  return 0;
}
console.log(check());
