// §10.2.10 SetFunctionLength counts the parameters the user wrote. A
// promoted function expression / object-literal method carries a
// synthetic `__this` receiver param, and the lifted-closure registry
// row filtered only the other synthetic name (`__env`) — so every
// promoted face read one too high, while the class-method row next
// door had been filtering both all along.

const A: any = function (n: number, m: number) { (this as any).n = n + m };
console.log(1, A.length);

// this-free control: same shape, no receiver param, must not move
const B: any = function (n: number, m: number) { return n + m };
console.log(2, B.length);

const o: any = {
  f: function (n: number) { (this as any).n = n },
  g(n: number, m: number) { return (this as any).x + n + m },
  h(n: number) { return n },
  get p(): any { return (this as any).q },
  q: 4,
};
console.log(3, o.f.length, o.g.length, o.h.length);
console.log(4, Object.getOwnPropertyDescriptor(o, "p").get.length, o.p);

// class method / constructor / arrow: already correct, pinned here so
// the fix is not confused for a change in their rows. (A STATIC
// method's `.length` reads `undefined` today — a different row, left
// out so this case stays about the one that moved.)
class K {
  v: number = 1;
  m(a: number, b: number): number { return (this as any).v + a + b }
}
console.log(5, (K.prototype as any).m.length, new K().m.length);

class C1 { constructor(a: number, b: number) { (this as any).a = a + b } }
class C0 { }
console.log(6, C1.length, C0.length);

const ar: any = (n: number, m: number) => n + m;
console.log(7, ar.length);

// a default stops the count before the receiver question arises
const D: any = function (n: number, m: number = 2) { (this as any).n = n + m };
console.log(8, D.length);
