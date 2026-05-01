// Integration: array + object destructuring with mixed types.
// Exercises tr's `Stmt::Multi`-based destructuring desugar.
type Point = { x: number, y: number };
type Named = { name: string, age: number };

function check(): number {
  // Array destructuring.
  let xs: number[] = [10, 20, 30];
  let [a, b, c] = xs;
  if (a !== 10) { throw "#1"; }
  if (b !== 20) { throw "#2"; }
  if (c !== 30) { throw "#3"; }

  // Skip middle would need spec extension; use nested arrays.
  let ys: number[] = [100, 200];
  let [p, q] = ys;
  if (p !== 100) { throw "#4"; }
  if (q !== 200) { throw "#5"; }

  // Object destructuring.
  let pt: Point = { x: 5, y: 7 };
  let { x, y } = pt;
  if (x !== 5) { throw "#6"; }
  if (y !== 7) { throw "#7"; }

  let n: Named = { name: "alice", age: 30 };
  let { name, age } = n;
  if (name !== "alice") { throw "#8"; }
  if (age !== 30) { throw "#9"; }

  // Sequential destructure of two structs.
  let s1: Point = { x: 1, y: 2 };
  let s2: Point = { x: 3, y: 4 };
  let { x: x1, y: y1 } = s1;
  let { x: x2, y: y2 } = s2;
  if (x1 + x2 !== 4) { throw "#10"; }
  if (y1 + y2 !== 6) { throw "#11"; }
  return 0;
}
console.log(check());
