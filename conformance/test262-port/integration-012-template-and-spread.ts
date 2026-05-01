// Integration: template literals + array spread interplay. Exercises
// the lex-time recursive tokenization for `${...}` plus the ssa-lower
// spread fast path (single-alloc cap = literal_count + spread.length).
type Pt = { x: number, y: number };

function check(): number {
  let name = "world";
  let count: number = 3;
  let g = `hello ${name}, count=${count}`;
  if (g !== "hello world, count=3") { throw "#1"; }

  // Template + arithmetic + nested member access.
  let p: Pt = { x: 5, y: 7 };
  let s = `(${p.x}, ${p.y}) sum=${p.x + p.y}`;
  if (s !== "(5, 7) sum=12") { throw "#2"; }

  // Array spread combinations.
  let a: number[] = [1, 2];
  let b: number[] = [4, 5];
  let c = [...a, 3, ...b];
  if (c.length !== 5) { throw "#3"; }
  if (c[0] !== 1) { throw "#4"; }
  if (c[2] !== 3) { throw "#5"; }
  if (c[4] !== 5) { throw "#6"; }

  // Spread of a spread (concat-like).
  let d = [...a, ...a, ...a];
  if (d.length !== 6) { throw "#7"; }
  if (d[0] !== 1) { throw "#8"; }
  if (d[5] !== 2) { throw "#9"; }

  // Empty + spread.
  let empty: number[] = [];
  let e = [...empty, 99, ...empty];
  if (e.length !== 1) { throw "#10"; }
  if (e[0] !== 99) { throw "#11"; }
  return 0;
}
console.log(check());
