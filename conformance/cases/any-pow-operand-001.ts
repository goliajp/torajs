// S2.43 — `**` accepts an Any operand on either side: ssa_lower
// routes through the anyv arith kernel (op 4), ES §13.6 ToNumber
// both sides then Number::exponentiate. Compound `**=` rides the
// same lane. Previously the checker rejected every Any operand
// ("requires matching number or bigint operands").
const a: any = 2;
console.log(a ** 5);
console.log(a ** 0.5);
const b: any = "3";
console.log(b ** 2);
const c: any = 2;
console.log(3 ** c);
const u: any = undefined;
console.log(u ** 2);
const n: any = null;
console.log(n ** 3);
const t: any = true;
console.log(t ** 5);
const neg: any = -1;
console.log(neg ** Infinity);
let p: any = 6;
p **= 2;
console.log(p);
class C {
  v;
  get x() {
    return this.v;
  }
  set x(val) {
    this.v = val;
  }
  run() {
    this.v = 2;
    this.x **= 5;
    return this.v;
  }
}
console.log(new C().run());
