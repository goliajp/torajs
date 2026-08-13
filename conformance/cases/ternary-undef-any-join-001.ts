// A ternary with one `undefined` branch and one already-`any` branch.
// The S2.27 wedge that boxes an undefined branch expr-aware only fires
// when NEITHER side is Any, so this shape fell through to the plain
// mixed-Any box, which tags a compile-time ConstPtrNull as ANY_NULL —
// `typeof` answered "object" and the value serialized as `null`.
const pick = function (drop: boolean, v: any): any {
  return drop ? undefined : v;
};
console.log(typeof pick(true, 2));
console.log(typeof pick(false, 2));
console.log(pick(false, "kept"));

// Either side of the conditional.
const pick2 = function (keep: boolean, v: any): any {
  return keep ? v : undefined;
};
console.log(typeof pick2(false, 2));
console.log(typeof pick2(true, 7));

// The statement form was already right — it is the contrast that
// pinned the root cause to the ternary join, not to return coercion.
const pick3 = function (drop: boolean, v: any): any {
  if (drop) {
    return undefined;
  }
  return v;
};
console.log(typeof pick3(true, 2));

// In a value position, not just a return.
function idAny(v: any): any {
  return v;
}
const anyVal: any = 5;
const joined: any = anyVal > 1 ? undefined : anyVal;
console.log(typeof joined);
console.log(typeof idAny(anyVal > 99 ? undefined : anyVal));

// The `&&` / `||` mixed-Any widen had the identical hole — the same
// plain box in `ssa_lower_logical`'s slot store.
const zero: any = 0;
const one: any = 1;
console.log(typeof (zero || undefined));
console.log(typeof (one && undefined));
console.log(typeof (zero ?? undefined));
console.log(typeof (one || undefined));

// The reason this matters here — the MDN spelling of a replacer that
// drops a key (§25.5.2.2 step 3: an undefined answer omits it).
console.log(JSON.stringify({ a: 1, b: 2 }, function (k: string, v: any) {
  return k === "b" ? undefined : v;
}));
console.log(JSON.stringify([1, 2], function (k: string, v: any) {
  return k === "0" ? undefined : v;
}));
