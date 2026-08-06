// §7.1.17 step 7 — ToString(BigInt) converts the value directly and
// looks nothing up, so a patched `BigInt.prototype.toString` is
// invisible to every IMPLICIT coercion and visible only to an
// EXPLICIT method call. Both halves matter: tr used to route the
// implicit coercions through the method dispatcher (correct answer,
// observable lookup) while the explicit call missed the patch
// entirely because the BigInt kernel answered ahead of it.
const B: any = BigInt;
B.prototype.toString = function () {
  return "PATCHED";
};
B.prototype.toLocaleString = function () {
  return "PATCHED_L";
};

const n: any = 1n;

// implicit — every one of these is §7.1.17, not a property lookup
console.log("template   :", `${n}`);
console.log("String()   :", String(n));
console.log("concat-rhs :", "" + n);
console.log("concat-lhs :", n + "");
console.log("arr-join   :", [n, n].join("-"));
// §22.1.3 generic ToString(this) — the route test262 checks via
// String.prototype.isWellFormed.call(1n)
console.log("str-generic:", (String.prototype as any).padStart.call(n, 3, "0"));
console.log("isWellForm :", (String.prototype as any).isWellFormed.call(n));

// explicit — a real property lookup, so the patch answers
console.log("explicit   :", n.toString());
console.log("explicit-L :", n.toLocaleString());
