// Two unrelated classes declare a method of the same name. A call on a
// receiver typed as one of them answers that class's return: a
// fraction stored into a number binding stays a fraction, through the
// class itself and through a subclass that inherits the method.
class P {
  m(): number {
    return 1.5;
  }
}
class Q {
  m(): number {
    return 3;
  }
}
class P2 extends P {}
const k: number = new P().m();
const j: number = new P2().m() * 2;
let t = 0;
for (const p of [new P(), new P2()]) t += p.m();
console.log(k, j, t, new Q().m());
