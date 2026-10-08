// a generic class that declares no constructor gets the implicit one,
// which forwards its arguments to the nearest ancestor constructor
class Box<T> {
  v: T
  n = 7
  constructor(v: T) { this.v = v }
  get val(): T { return this.v }
}
class Plain<T> extends Box<T> {}
const p = new Plain<string>("hi")
console.log(p.v, p.n, p.val)
class Wrap<T> extends Box<T> { tag(): string { return "w" + this.v } }
const w = new Wrap(2)
console.log(w.tag(), w.val)
class Mid<U> extends Box<U> { extra = 1 }
class Leaf<K> extends Mid<K> {}
const l = new Leaf<number>(5)
console.log(l.v, l.n, l.extra)
class Many<T> extends Box<T[]> {}
const m = new Many<number>([1, 2])
console.log(m.v.length, m.n)
class Pair<A, B> { a: A; b: B; constructor(a: A, b: B) { this.a = a; this.b = b } }
class Swap<X> extends Pair<string, X> {}
const s = new Swap<boolean>("k", true)
console.log(s.a, s.b)
class Bare<T> { t = 3 }
class BareKid<T> extends Bare<T> {}
console.log(new BareKid<number>().t)
