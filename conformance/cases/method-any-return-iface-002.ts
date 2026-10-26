// A method that returns an any value behind an interface, in three
// more spellings: the interface declares the member as a function-typed
// property, an object literal fills it with method shorthand, and a
// member is read as a value through the interface. An interface of
// the same shape as one read that way is one type with it, so the
// read interface is spelled with a member of its own.
const raw: any = JSON.parse('{"x": 1.5, "y": "s"}');

interface J {
  m: () => number;
}
class P implements J {
  v: any;
  constructor(v: any) {
    this.v = v;
  }
  m(): number {
    return this.v.x;
  }
}
class R implements J {
  m(): number {
    return 2.25;
  }
}
const js: J[] = [new P(raw), new R()];
for (const j of js) console.log(j.m());
function viaJ(j: J): number {
  return j.m();
}
console.log(viaJ(new P(raw)), viaJ(new R()));

interface I {
  m(): number;
}
class S implements I {
  v: any;
  constructor(v: any) {
    this.v = v;
  }
  m(): number {
    return this.v.y;
  }
}
const short: I = {
  m() {
    return 4.5;
  },
};
function mkLit(): I {
  return {
    m(): number {
      return 0.25;
    },
  };
}
const is: I[] = [new S(raw), short, mkLit()];
for (const i of is) console.log(i.m());
function viaI(i: I): number {
  return i.m();
}
console.log(viaI(new S(raw)), viaI(short), viaI(mkLit()));

interface K {
  p(): number;
}
class T implements K {
  v: any;
  constructor(v: any) {
    this.v = v;
  }
  p(): number {
    return this.v.x;
  }
}
function read(k: K): number {
  const h = k.p;
  return h();
}
const arrow: K = { p: () => 4.5 };
const ks: K[] = [new T(raw), arrow];
for (const k of ks) console.log(k.p());
console.log(read(arrow), arrow.p());
const h2 = arrow.p;
console.log(h2() + 1);
