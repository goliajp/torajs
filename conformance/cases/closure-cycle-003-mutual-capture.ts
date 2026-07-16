// RFC 20260717 closure-env-cycle knife 4 — mutual capture through a
// boxed binding: b's env captures the box of a, the arrow assigned
// into a captures b, closing the loop across two envs.
function mutual(i: number): number {
  let a: any = null;
  const b = () => (a === null ? -1 : i);
  a = () => b;
  const inner: any = a();
  return b() + (typeof inner === "function" ? 1 : 0);
}
let total = 0;
for (let i = 0; i < 3000; i++) {
  total += mutual(i);
}
console.log(total);
