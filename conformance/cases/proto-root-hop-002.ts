// 521-05 — the call-channel twin of 517-07. A method patched onto
// Object.prototype reaches receivers whose own family prototype does
// not carry it (§10.1.9.2). The walk used to stop at the family
// singleton, and worse, returned early when that singleton had never
// been minted — a program that patches Object.prototype and never
// touches Array.prototype leaves it unminted, so the root leg was
// never even reached.
//
// The class-instance and dynobj receivers are NOT covered here: their
// method calls route through cell_method_inheriting rather than the
// proto-patch consult, and are registered as 521-06.
(Object.prototype as any).mm = function () {
  return 9;
};

const a: any = [1, 2];
console.log("arr", a.mm());

const s: any = "ab";
console.log("string", s.mm());

const f: any = () => 1;
console.log("closure", f.mm());

const m: any = new Map();
console.log("map", m.mm());

// the family's own surface still wins over the root
(Object.prototype as any).join = function () {
  return "ROOT";
};
console.log("family-wins", a.join("-"));

// an entry storing undefined is a real entry, not an absence
(Object.prototype as any).zz = undefined;
console.log("undef-entry", typeof (a as any).zz);

// a genuinely absent name still throws
try {
  a.nope();
} catch (e: any) {
  console.log("absent threw");
}
