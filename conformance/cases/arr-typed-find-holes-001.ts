// The find family reads every index with [[Get]] (§23.1.3.9 — no
// HasProperty step), so a hole reaches the predicate as undefined: for
// a number[] (the slot holds the sentinel), a string[] and an object[]
// (a NULL slot), through a typed or an `any` parameter, and whether the
// holes were made next to the call or in another function.
function mk(): number[] {
  let a = [1.5, 2.5];
  a[4] = 3.5;
  return a;
}
console.log(mk().findIndex((v) => v === undefined), mk().findLastIndex((v) => v === undefined));
console.log(mk().find((v) => v === undefined), mk().findLast((v) => v !== 3.5));
console.log(mk().findIndex((v) => v > 3), mk().find((v) => Number.isNaN(v)));
const a = mk();
console.log(a.findIndex((v) => {
  console.log(typeof v, v === undefined, String(v));
  return false;
}));
let b = [1.5, 2.5];
b[4] = 3.5;
console.log(b.findIndex((v) => v === undefined));
const s = ["a", "b"];
s[3] = "d";
console.log(s.findIndex((v) => v === undefined), s.findLastIndex((v) => typeof v === "undefined"));
const ss = ["x"];
ss[2] = "z";
console.log(ss.find((v) => v === undefined), ss.findLast((v) => v !== undefined), ss.every((v) => typeof v === "string"));
const o = [{ k: 1 }];
o[2] = { k: 3 };
console.log(o.findIndex((e) => e === undefined), o.find((e) => e !== undefined && e.k > 2)?.k, o.some((e) => e === undefined));
function mko() {
  const o = [{ k: 1 }];
  o[2] = { k: 3 };
  return o;
}
console.log(mko().findIndex((e) => e === undefined));
console.log([1, 2, 3].findIndex((v) => v === 2), [1.5].find((v) => v > 1));
