// RFC 20260721-string-proto-cluster G10 face lock — assigning onto a
// builtin prototype singleton through an any binding works, and
// §7.1.17 ToString(primitive) does NOT consult the patched proto
// (the patch must not be called); ToString(Symbol) still throws.
let bp: any = Boolean.prototype;
bp.toString = function () {
  throw new Error("nope");
};
let r = (String.prototype.isWellFormed as any).call(true);
delete bp.toString;
console.log(r);
let np: any = Number.prototype;
np.toString = function () {
  throw new Error("nope-n");
};
let r2 = (String.prototype.isWellFormed as any).call(1);
delete np.toString;
console.log(r2);
try {
  (String.prototype.isWellFormed as any).call(Symbol());
  console.log("no-throw");
} catch (e) {
  console.log("threw");
}
