// Reflect.construct over bound layers: a newTarget naming one of the
// layers is swapped for that layer's target on the way down, so the
// object's prototype comes from the constructor actually reached.
let seen: any
function A() {
  seen = new.target
}
const B = A.bind(null)
const C = B.bind(null)
const D = C.bind(null)

const c1 = Reflect.construct(C, [], B)
console.log(seen === A, Object.getPrototypeOf(c1) === A.prototype)
const c2 = Reflect.construct(D, [], C)
console.log(seen === A, Object.getPrototypeOf(c2) === A.prototype)
const c3 = Reflect.construct(D, [], D)
console.log(seen === A, Object.getPrototypeOf(c3) === A.prototype)

function X() {}
X.prototype = { tag: "x" }
const c4: any = Reflect.construct(C, [], X)
console.log(seen === X, c4.tag)

const N = function () {}.bind(null)
const c5 = Reflect.construct(C, [], N)
console.log(seen === N, Object.getPrototypeOf(c5) === Object.prototype)
