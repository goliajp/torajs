// A builtin prototype method is an own property of that prototype, so
// deleting it leaves nothing for an instance to inherit. torajs's
// builtin surface is virtual rather than a real entry table, so the
// method-call dispatcher has to consult the delete tombstone the way
// the value-read face already does — otherwise the surface shows back
// through and the call answers as if nothing happened.

const m: any = new Map([[1, "native"]]);
const s: any = new Set([1]);

console.log("before  ", m.get(1), s.has(1));

delete (Map.prototype as any).get;
try {
  console.log("deleted: no throw", m.get(1));
} catch (e: any) {
  console.log("deleted:", e instanceof TypeError);
}

// the rest of the prototype is untouched — this is one property, not
// a switch on the whole surface
console.log("siblings", m.has(1), m.size, s.has(1));

// a later assignment revives it: the own entry is probed before the
// tombstone is consulted, so no explicit clear is involved
(Map.prototype as any).get = function (k: any) {
  return "REVIVED";
};
console.log("revived ", m.get(1));

// deleting a method that was patched first behaves the same way — the
// patch bit is sticky, the own probe misses, and the tombstone wins
(Set.prototype as any).has = function () {
  return "PATCHED";
};
console.log("patched ", s.has(1));
delete (Set.prototype as any).has;
try {
  console.log("re-deleted: no throw", s.has(1));
} catch (e: any) {
  console.log("re-deleted:", e instanceof TypeError);
}
