// RFC 20260722-find-miss chunk B — a member read through a
// find/findLast miss is a catchable TypeError (bun: undefined is
// not an object), not a deref of the sentinel header.
type P = { n: number };
const xs: P[] = [{ n: 1 }];
const r = xs.find((x) => x.n === 99);
try {
  console.log(r.n);
} catch (e) {
  console.log("caught field");
}
const ys: number[][] = [[1]];
const ry = ys.find((a) => a.length === 9);
try {
  console.log(ry.length);
} catch (e) {
  console.log("caught len");
}
try {
  console.log(ry[0]);
} catch (e) {
  console.log("caught idx");
}
console.log("end");
