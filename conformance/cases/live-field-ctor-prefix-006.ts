// `this` is the instance under construction only inside a
// constructor; in a method it is whatever the caller passed. A field
// a method stores its receiver into keeps its read guard, and a field
// a constructor stores `this` into reads it back. (A method value
// called with an undefined receiver would store undefined; tr refuses
// that call with a TypeError instead, so the shape is not observable
// here.)
class Owner {
  n: number
  constructor(n: number, h: Holder | null) {
    this.n = n
    if (h !== null) h.owner = this
  }
  attach(h: Holder) {
    h.owner = this
  }
}
class Holder {
  owner: Owner
  constructor(o: Owner) {
    this.owner = o
  }
}
const h = new Holder(new Owner(1, null))
console.log(h.owner.n)
new Owner(2, h)
console.log(h.owner.n)
const o = new Owner(3, null)
o.attach(h)
console.log(h.owner.n)
