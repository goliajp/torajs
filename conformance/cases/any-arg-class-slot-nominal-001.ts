// an instance of another class with the same fields, handed through
// `any` to a parameter typed by a class, keeps its own methods and
// its own identity: the typed body may not run as if it were the
// parameter's class
class P {
  x = 1;
  m() {
    return "P" + this.x;
  }
}
class Q {
  x = 1;
  m() {
    return "Q" + this.x;
  }
}
// a subclass that keeps `m` is still a `P`
class R extends P {
  y = 2;
  n() {
    return "R";
  }
}
function f(p: P) {
  return p.m() + ":" + (p instanceof P) + ":" + (p instanceof Q);
}
const g: any = f;
const xs: any[] = [new P(), new Q(), new R()];
for (const x of xs) console.log(g(x));
console.log(f(new P()), f(new R()));

function each(xs: any[], cb: any) {
  const out: string[] = [];
  for (const x of xs) out.push(cb(x));
  return out.join(",");
}
console.log(each([new Q(), new P()], f));
const holder: any = { v: new Q() };
console.log(g(holder.v));
const bag: any = {};
bag.k = new Q();
console.log(g(bag.k));
console.log(g({ x: 1, m() { return "lit"; } }));
console.log([new Q()].map(f as any).join());

class Box {
  k(p: P) {
    return p.m();
  }
}
const b: any = new Box();
console.log(b.k(new Q()), b.k(new R()), new Box().k(new P()));
