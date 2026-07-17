// RFC 20260717-namedfn-canonical-cell chunk 1 — a top-level named fn
// used as a value answers ONE singleton cell from every forwarder
// site (ES fn objects are created once at declaration instantiation):
// two reads compare equal, expando writes are program-wide visible,
// and cross-object accessor faces share the cell. (The bare-Ident
// `t === getX` eq-operand axis is a recorded follow-up.)
function getX(): number {
  return 7;
}
const t: any = getX;
const u: any = getX;
console.log(t === u);
const o: any = { cb: getX };
console.log(o.cb === t);
t.tag = "shared";
console.log(u.tag, o.cb.tag);
console.log(getX());
const a1: any = {};
const a2: any = {};
Object.defineProperty(a1, "x", { get: getX });
Object.defineProperty(a2, "x", { get: getX });
const g1: any = Object.getOwnPropertyDescriptor(a1, "x").get;
const g2: any = Object.getOwnPropertyDescriptor(a2, "x").get;
console.log(g1 === g2);
console.log(a1.x + a2.x);
