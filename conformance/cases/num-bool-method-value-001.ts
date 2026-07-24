// Rotation 207 — RFC 20260725-str-method-value-reify recorded the
// Number / Boolean receiver family as a follow-on knife. A builtin
// method read as a VALUE off a Number- or Boolean-typed receiver
// (`const f = n.toString`) now reifies the same interned mid-cell the
// String receiver does, minted against its own prototype family so
// the runtime's brand gate and the family-aware `length` both land.

const n = 255;
const f = n.toString;
console.log("A", typeof f);
console.log("B", f.name);
console.log("C", f.length);
console.log("D", f.call(255, 16));
console.log("E", f.call(255));
console.log("F", f.apply(255, [2]));

const x = 3.14159;
const g = x.toFixed;
console.log("G", g.name);
console.log("H", g.length);
console.log("I", g.call(3.14159, 2));

const b = true;
const h = b.toString;
console.log("J", h.name);
console.log("K", h.length);
console.log("L", h.call(false));

const v = n.valueOf;
console.log("M", v.call(42));

// Brand gate reaches the reified instance-receiver cell too.
try {
  console.log("N no-throw", h.call({}));
} catch (e) {
  console.log("N", e instanceof TypeError);
}
try {
  console.log("O no-throw", v.call("nope"));
} catch (e) {
  console.log("O", e instanceof TypeError);
}

// bind keeps the family (rotation 207 chunk 2).
console.log("P", f.bind(255)(16));
console.log("Q", f.bind(255).length);

// The String receiver family is unchanged.
const s = "abcdef";
const m = s.slice;
console.log("R", m.name);
console.log("S", m.length);
console.log("T", m.call("abcdef", 1, 3));
console.log("U", s.toString.length);
