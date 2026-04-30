// Adapted from test262: nested while-loops with break/continue.
function check(): number {
  let count: number = 0;
  let i: number = 0;
  while (i < 5) {
    let j: number = 0;
    while (j < 5) {
      if (j === 3) { j = j + 1; continue; }
      count = count + 1;
      j = j + 1;
    }
    i = i + 1;
  }
  // 5 outer × (5 - 1 skipped) = 20
  if (count !== 20) { throw "#1"; }
  return 0;
}
console.log(check());
