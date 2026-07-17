// String/prototype S15.5.4.x_A1_T2 family — a reified builtin
// method stored on a wrapper's expando must run against the
// WRAPPER receiver with the §22.1.3 generic ToString(this) coerce
// (own-property order: the expando wins over the view-through
// surface, which delegated straight to the inner primitive's arm
// and answered not-a-function).

const inst: any = new Boolean(false);
inst.charAt = (String.prototype as any).charAt;
console.log(inst.charAt(0)); // f
console.log(inst.charAt(1)); // a
console.log(inst.charAt(2)); // l

inst.match = (String.prototype as any).match;
const m: any = inst.match("false");
console.log(m[0]); // false

// arbitrary expando names dispatch the same way
const s: any = new String("hi");
s.up = (String.prototype as any).toUpperCase;
console.log(s.up()); // HI

// a Number-family reified method on a Number wrapper
const n: any = new Number(3.75);
n.fmt = (Number.prototype as any).toFixed;
console.log(n.fmt(1)); // 3.8

// a user closure on the expando still runs (receiver channel)
let seen = "";
s.tag = function (x: any) { seen = "t" + x; return 1; };
console.log(s.tag(7), seen); // 1 t7

// the untouched view-through surface keeps working
console.log((new String("abc") as any).toUpperCase()); // ABC
console.log("done");
