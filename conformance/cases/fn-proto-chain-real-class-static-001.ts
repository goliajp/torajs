// A function value re-parented under a REAL class (405-01 face 2,
// probe p3): `Object.setPrototypeOf(F, P)` puts the class object on
// F's user [[Prototype]] chain, so F.s() resolves P's statics — own
// and inherited — with F as the receiver. Pre-fix the walk treated
// any non-closure ancestor as a boundary and threw TypeError.
class P { static s() { return 5 } static t() { return 20 } }
class Q extends P { static t() { return 21 } }
const F: any = function () {}
Object.setPrototypeOf(F, P)
console.log(F.s(), F.t())
const G: any = function () {}
Object.setPrototypeOf(G, Q)
console.log(G.s(), G.t())
const H: any = function () {}
Object.setPrototypeOf(H, P)
console.log(typeof H.missing)
