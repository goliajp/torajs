// `p?: T` on a class member — §9.2 makes it `p: T | undefined`, the
// same shape the parameter position has always desugared to. The
// object-type spelling `{ a?: T }` already worked; the class one did
// not parse at all.
class K {
  p?: string;
  n?: number;
  b?: boolean;
}
const k = new K();
console.log(k.p, k.n, k.b);
console.log(k.p === undefined, k.n === undefined, k.b === undefined);
console.log(typeof k.p, typeof k.n, typeof k.b);
console.log(k.p === null, k.n === null);

// an initializer still wins, and a bare `p?` keeps the `any` lane
class M {
  q?: number = 5;
  s?: string = "x";
  r?;
}
const m = new M();
console.log(m.q, m.s, m.r);

// the longhand agrees with the shorthand
class L {
  p: string | undefined = undefined;
}
const l = new L();
console.log(l.p, l.p === undefined, typeof l.p);

// optional fields are ordinary own properties
console.log(JSON.stringify(Object.keys(k)));
console.log(JSON.stringify(Object.getOwnPropertyNames(m)));

// and they still take a value
class W {
  v?: number;
}
const w = new W();
w.v = 3;
console.log(w.v, typeof w.v);
