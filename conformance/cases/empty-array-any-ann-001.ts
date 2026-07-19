// L3b 2026-07-12 recorded face, probe-cleared 2026-07-20: an
// any-annotated binding admits an empty `[]` literal (promoted to
// the Arr<Any> lane) across let-decl, fn-scope, as-cast, call-arg
// and re-assignment shapes. Fixture locks the behavior in.
const empty: any = [];
empty.push(1);
empty.push("two");
console.log(empty.length, JSON.stringify(empty));
let e2: any = [];
console.log(e2.length);
function f() {
  const inner: any = [];
  inner.push(9);
  return inner.length;
}
console.log(f());
const cast = [] as any;
console.log(JSON.stringify(cast));
const g = (x: any) => x.length;
console.log(g([]));
let reassign: any = [];
reassign = [1, 2];
console.log(reassign.length);
