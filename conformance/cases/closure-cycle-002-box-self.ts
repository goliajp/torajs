// RFC 20260717 closure-env-cycle knife 4 — capture-box self cycle:
// `a` is reassigned after capture so it lives in a shared box; the
// closure's env holds the box, the box ends up holding the closure.
function loop(i: number): number {
  let a: any = i;
  const f = () => a;
  a = f;
  return typeof f() === "function" ? 1 : 0;
}
let total = 0;
for (let i = 0; i < 3000; i++) {
  total += loop(i);
}
console.log(total);
