// RFC 20260710-optional-undefined-repr C2a — undefined literal into
// a fn-typed (FnSig) struct slot stores the sentinel oddball:
// strict-eq splits undefined from null, single-arg and multi-arg
// print render undefined/null/[Function: name], truthiness stays
// falsy, and null-init / named-fn-init slots keep exact behavior.
type O = { cb?: () => string };
const o: O = { cb: undefined };
console.log(o.cb === undefined, o.cb === null);
console.log(o.cb);
if (o.cb) {
  console.log("truthy");
} else {
  console.log("falsy");
}
function named(): string {
  return "n";
}
const p: O = { cb: named };
console.log(p.cb === undefined, p.cb === null);
console.log(p.cb);
console.log("mix:", o.cb, p.cb);
const q: O = { cb: named };
q.cb = undefined;
console.log(q.cb === undefined, q.cb === null, q.cb);
const r: { cb: (() => string) | null } = { cb: null };
console.log(r.cb === null, r.cb === undefined, r.cb);
const both: { tag?: string; cb?: () => string } = { tag: undefined, cb: undefined };
console.log(both.tag === undefined, both.cb === undefined);
console.log("both:", both.tag, both.cb);
