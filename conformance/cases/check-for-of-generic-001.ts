// P5.3 — generic `for (let v of <expr>) body` via Stmt::ForOf
// substrate. Parser hoists non-Ident src into a fresh let and
// pre-builds `elem_expr = src[i]`. ssa_lower routes the element
// load through existing Expr::Index lowering so Type::Any boxing,
// Substr borrow, and typed Array<T> all handle uniformly.

// Number array — copy elem, no rc bookkeeping.
const nums: number[] = [10, 20, 30];
let s = 0;
for (const n of nums) { s += n; }
console.log(s);   // 60

// String array (refcounted elem).
const names = ["alpha", "beta", "gamma"];
let buf = "";
for (const w of names) { buf = buf + w; }
console.log(buf); // alphabetagamma

// Mutable `let` source (local alloca path).
let xs: number[] = [1, 2, 3, 4];
let sq = 0;
for (const n of xs) { sq = sq + n * n; }
console.log(sq);  // 30

// Inline literal src — parser hoists to __forof_src_<id>.
let acc = 0;
for (const x of [7, 11, 13]) { acc += x; }
console.log(acc); // 31

// break + continue.
let part = 0;
for (const v of nums) {
  if (v === 30) break;
  if (v === 10) continue;
  part += v;
}
console.log(part); // 20

// Nested for-of.
const rows: number[][] = [[1, 2], [3, 4], [5, 6]];
let grid = 0;
for (const row of rows) {
  for (const v of row) {
    grid += v;
  }
}
console.log(grid); // 21

// Array<Any> source — Type::Any boxing path.
const mixed: any[] = [42, "hi", true, 3.14];
for (const v of mixed) {
  console.log(v);
}
