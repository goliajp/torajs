// RFC 20260824-s2-5 刀 4 A4' — scalar-kind arrays drop through a
// kernel whose only slow legs (a props bag to release, a subclass
// envelope to unwind) sit behind link seams; this program exercises
// both legs, so both must stay bound to their real drop paths (and
// nothing may leak or double-free across the repeated drops).
class Nums extends Array<number> {
  total(): number {
    let t = 0;
    for (let i = 0; i < this.length; i++) t += this[i];
    return t;
  }
}
function tagged(n: number): number {
  const xs: number[] = [n, n + 1, n + 2];
  (xs as any).tag = "t" + n;
  return xs.length + (xs as any).tag.length;
}
function sub(n: number): number {
  const s = new Nums();
  s.push(n, n * 2);
  return s.total();
}
let acc = 0;
for (let i = 0; i < 200; i++) {
  acc += tagged(i) + sub(i);
}
console.log(acc);
const plain: number[] = [1, 2, 3];
console.log(plain.length);
