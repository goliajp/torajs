// §21.1.3 / §22.1.3 — a wrapper object reads the same prototype
// methods its primitive does, and they are the same function objects.
// Reading one answered undefined while calling it worked.
const w: any = new Number(3);
console.log(typeof w.toFixed, typeof w.toPrecision, w.toPrecision(2), w.toFixed === Number.prototype.toFixed);
const s: any = new String("ab");
console.log(typeof s.padStart, s.padStart(3, "-"), s.padStart === String.prototype.padStart);
const b: any = new Boolean(true);
console.log(typeof b.toString, b.toString());
class N extends Number { }
const n: any = new N(4);
console.log(typeof n.toPrecision, n.toPrecision(3), n.toFixed(1));
