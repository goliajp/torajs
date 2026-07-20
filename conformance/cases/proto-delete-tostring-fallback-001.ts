// RFC 20260721-string-proto-cluster 刀 6 (G9) — deleting a builtin
// prototype's family toString retires the primitive-identity face;
// the call inherits Object.prototype.toString (§20.1.3.6 badge).
// A monkey-patch restore wins again (own entry before tombstone).
delete (String.prototype as any).toString;
console.log((String.prototype as any).toString());
delete (Boolean.prototype as any).toString;
console.log((Boolean.prototype as any).toString());
console.log((Number.prototype as any).toString());
(String.prototype as any).toString = function () {
  return "patched";
};
console.log((String.prototype as any).toString());
