// Symbol receivers on the any lane: toString / toLocaleString /
// valueOf / description / reify / miss (ES §20.4.3)
const s: any = Symbol("hello");
console.log(s.toString());
console.log(typeof s.toString());
console.log(s.toLocaleString());
console.log(s.valueOf() === s);
console.log(s.description);

const u: any = Symbol();
console.log(u.toString());
console.log(u.description);

const v: any = Symbol.iterator;
console.log(v.toString());

const r: any = Symbol.for("reg");
console.log(r.toString(), r.description);

// reading a method as a value hands out the function cell
const m = s.toString;
console.log(typeof m);

// unknown method: optional-call short-circuits, plain call throws
console.log(s.nope?.());
try {
  s.nope();
} catch (e: any) {
  console.log("miss caught:", e instanceof TypeError);
}
