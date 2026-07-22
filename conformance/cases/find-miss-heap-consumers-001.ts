// RFC 20260722-find-miss chunk C — heap-slot nullish loose-eq,
// typed-lane composite print, and closure-miss call/member guards.
type P = { n: number };
const xs: P[] = [{ n: 1 }];
const o = xs.find((x) => x.n === 99);
console.log(o == null, o == undefined, o != null);
const hit = xs.find((x) => x.n === 1);
console.log(hit == null, hit != null);
const box = [o];
console.log(box);
type W = { inner?: P };
const w: W = {};
const u = w.inner;
console.log(u == null, u != null);
const fs = [(x: number) => x + 1];
const rf = fs.find((f) => false);
try {
  console.log(rf(1));
} catch (e) {
  console.log("caught call");
}
try {
  console.log(rf.length);
} catch (e) {
  console.log("caught len");
}
console.log("end");
