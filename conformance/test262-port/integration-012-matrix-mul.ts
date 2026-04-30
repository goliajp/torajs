// Integration: 3x3 matrix multiply — exercises nested arrays, three
// nested for-loops, accumulator pattern.
function multiply(a: number[][], b: number[][]): number[][] {
  let out: number[][] = [
    [0, 0, 0],
    [0, 0, 0],
    [0, 0, 0],
  ];
  // Workaround: tr lacks `arr[i] = v` for a 2D array slot, so we
  // build each row fresh and assemble at the end via a flat helper.
  let rows: number[][] = [];
  for (let i: number = 0; i < 3; i = i + 1) {
    let row: number[] = [];
    for (let j: number = 0; j < 3; j = j + 1) {
      let s: number = 0;
      for (let k: number = 0; k < 3; k = k + 1) {
        s = s + a[i][k] * b[k][j];
      }
      row.push(s);
    }
    rows.push(row);
  }
  return rows;
}

function check(): number {
  let id: number[][] = [[1, 0, 0], [0, 1, 0], [0, 0, 1]];
  let m: number[][] = [[1, 2, 3], [4, 5, 6], [7, 8, 9]];
  let r = multiply(id, m);
  // identity * m = m
  if (r[0][0] !== 1) { throw "#1"; }
  if (r[1][1] !== 5) { throw "#2"; }
  if (r[2][2] !== 9) { throw "#3"; }
  if (r[2][0] !== 7) { throw "#4"; }
  return 0;
}
console.log(check());
