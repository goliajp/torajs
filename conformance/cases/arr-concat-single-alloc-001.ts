// rotation 552 — a typed Array.concat builds its product in one
// allocation (receiver + every argument summed, copied in, adopted per
// source) instead of a pairwise fold that leaked one intermediate per
// extra argument and every owned-temp argument (185-222MB / 600k, no
// throw needed). Views materialize per copied range, scalars append,
// owned temps release, a throwing later argument releases the earlier
// ones. Every shape answers what bun answers.
const s = (n: number): string => "v" + n;
const a = (n: number): number[] => [n, n + 1];
const as = (n: number): string[] => [s(n), s(n + 1)];
const boom = (): any => {
  throw new Error("x");
};
const N = 200;
let caught = 0;

let total = 0;
for (let i = 0; i < N; i++) {
  const r = a(i).concat(a(i), a(i));
  total += r.length + r[5];
}
console.log("num3", total, a(1).concat(a(1), a(1)).join(","));
console.log("num0", a(1).concat().join(","));
console.log("num1", a(1).concat(a(2)).join(","));
console.log("numsc", a(1).concat(9, a(2), 10).join(","));
const base = [1, 2];
const c = base.concat(base, base);
base.push(99);
console.log("alias", c.join(","), base.join(","));

total = 0;
for (let i = 0; i < N; i++) {
  const rs = as(i).concat(as(i), as(i));
  total += rs.length + rs[5].length;
}
console.log("str3", total, as(1).concat(as(1), as(1)).join(","));
const word = "hello";
const sp = word.split("");
console.log("views", sp.concat(as(1), word[4]).join(","), sp.length);
console.log("strsc", ["a"].concat("b", as(1), s(2)).join(","));
const f = [1.5, 2];
console.log("f64", f.concat(3, [4.25]).join(","));

for (let i = 0; i < N; i++) {
  try {
    a(i).concat(a(i), a(boom()));
  } catch (e) {
    caught++;
  }
}
for (let i = 0; i < N; i++) {
  try {
    as(i).concat(as(i), as(boom()));
  } catch (e) {
    caught++;
  }
}
console.log("throw", caught);
