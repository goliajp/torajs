// an array handed through `any` to a parameter typed as an array of a
// class holds instances of that class (or a subclass) in every
// element; an array of another class with the same fields does not,
// and each element keeps its own methods
class P {
  x = 1;
  m() {
    return "P";
  }
}
class Q {
  x = 1;
  m() {
    return "Q";
  }
}
class R extends P {
  y = 2;
}
function h(ps: P[]) {
  let s = "";
  for (const p of ps) s += p.m();
  return s;
}
const g: any = h;
const qa: Q[] = [new Q(), new Q()];
const ra: R[] = [new R()];
console.log(g(qa), g(ra), h([new P(), new R()]));
const mixed: any[] = [new P(), new Q()];
console.log(g(mixed));

// an interface slot takes any object with its fields
interface I {
  x: number;
}
function fi(i: I) {
  return i.x + 1;
}
const gi: any = fi;
console.log(gi(new Q()), gi(new P()), gi({ x: 5 }), fi({ x: 2 }));

// a class whose constructor parameter has no annotation mints its
// instances per specialization; they are still its instances
class C {
  v: number;
  constructor(v) {
    this.v = v;
  }
  m() {
    return "C" + this.v;
  }
}
function fc(c: C) {
  return c.m() + (c instanceof C);
}
const gc: any = fc;
console.log(gc(new C(3)), fc(new C(4)));
