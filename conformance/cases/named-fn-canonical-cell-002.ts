// RFC 20260717-namedfn-canonical-cell chunk 2 — the eq-operand axis:
// a bare (or as-cast) top-level named fn compared with === / !== is a
// value use of the fn object and answers the canonical singleton, so
// identity comparisons against every other value site hold (pre-fix
// the operand lowered to the raw FnSig code address and always
// compared unequal).
function getX(): number {
  return 7;
}
function other(): number {
  return 8;
}
const t: any = getX;
console.log(t === getX);
console.log(t !== getX);
console.log(t === other);
console.log(getX === getX);
console.log((getX as any) === getX);
const o: any = {};
Object.defineProperty(o, "x", { get: getX });
const g: any = Object.getOwnPropertyDescriptor(o, "x").get;
console.log(g === getX);
console.log(getX(), t(), o.x);
