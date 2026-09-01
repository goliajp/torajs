// 555-01 — a typed array literal is built in place: the block is
// allocated once the slot type is known and every element is stored
// the moment it is lowered, `len` advancing one slot at a time, the
// block itself the one parked temp. So a throw between two elements
// drops exactly the stored prefix, no element outlives its store,
// and a 65k-pair table no longer spills every value. The shapes the
// lane still has to answer: nested literals (anchor from the first
// element), empty nested literals under an Arr anchor, a Str slot
// holding `undefined`, an f64 anchor widening an i64 element, a
// Substr element materialized to an owned Str, shares of named
// bindings, and the throw-in-the-middle churn.
const m = [["a", "b"], ["c"], []];
console.log(m.length, m[0][1], m[1].length, m[2].length);
const e: string[][] = [[], []];
e[1].push("q");
console.log(e[0].length, e[1][0]);

const xs = ["a", undefined];
console.log(xs.length, xs[1] === undefined, String(xs[1]), xs.join("-"));

const ns = [1, 2.5, 3];
let t = 0;
for (const n of ns) t += n;
console.log(t, ns[0] / 2);
function f(): number {
  const a = [1, 2, 3];
  return a[1] + a.length;
}
console.log(f());

const s = "hello";
const parts = [s[1], "z", s[4]];
console.log(parts.join("|"), parts[0].length);

const pre = "P";
const rows = [[pre + "1", "x"], [pre + "2", "y"]];
console.log(rows.map((r) => r.join(":")).join(";"));
const big: number[] = [];
for (let i = 0; i < 5; i++) big.push(i * i);
const wrapped = [big, [7]];
console.log(wrapped[0].length, wrapped[1][0], wrapped[0][4]);
const shared = [pre, s, pre];
console.log(shared.join(""), pre, s);

function mk(i: number): string {
  return "s" + i;
}
function boom(i: number): number {
  if (i >= 0) throw new Error("x" + i);
  return i;
}
let caught = 0;
for (let i = 0; i < 300000; i++) {
  try {
    const a = [mk(i), mk(i + 1), String(boom(i))];
    caught -= a.length;
  } catch (e) {
    caught++;
  }
}
console.log(caught);
