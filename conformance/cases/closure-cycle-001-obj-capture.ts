// RFC 20260717 closure-env-cycle knife 4 — closure<->obj cycle:
// the env captures the instance, the instance's field holds the
// closure. 3000 iterations cross the 1024-candidate auto-collect
// threshold repeatedly; values must stay correct through collects.
class Node {
  f: any = null;
  tag: number = 0;
}
function mk(i: number): number {
  const n = new Node();
  n.tag = i;
  const f = () => n.tag;
  n.f = f;
  return f();
}
let total = 0;
for (let i = 0; i < 3000; i++) {
  total += mk(i);
}
console.log(total);
