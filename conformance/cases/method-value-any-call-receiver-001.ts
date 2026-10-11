// a class method read as a value and called through `any` takes its
// receiver from call / apply / bind (§20.2.3.1-3)
class K {
  v = 3
  m(x: number): number {
    return this.v + x
  }
}
const h: any = K.prototype.m
console.log(h.call(new K(), 4))
console.log(h.apply(new K(), [5]))
const k2 = new K()
k2.v = 10
console.log(h.call(k2, 1), h.call({ v: 100 }, 1))
class L extends K {
  w = 20
  n(s: string): string {
    return s + this.v + this.w
  }
}
const hn: any = L.prototype.n
console.log(hn.call(new L(), 'v='), h.call(new L(), 2))
const im: any = new K().m
console.log(im.call(k2, 2))
const bound = h.bind(k2)
console.log(bound(3))
