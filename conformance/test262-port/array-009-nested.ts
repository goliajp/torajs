// Adapted from test262: language/expressions/array-literal/* —
// nested-array case (`number[][]`). Verifies the multi-dim array layout.
function check(): number {
  let m: number[][] = [[1, 2, 3], [4, 5, 6], [7, 8, 9]];
  if (m.length !== 3) { throw "#1"; }
  if (m[0].length !== 3) { throw "#2"; }
  if (m[1][1] !== 5) { throw "#3"; }
  if (m[2][2] !== 9) { throw "#4"; }
  // sum of all elements = 45
  let total: number = 0;
  for (let i: number = 0; i < m.length; i = i + 1) {
    for (let j: number = 0; j < m[i].length; j = j + 1) {
      total = total + m[i][j];
    }
  }
  if (total !== 45) { throw "#5"; }
  return 0;
}
console.log(check());
