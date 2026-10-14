// a function that returns a function, called or constructed through
// a value of type any
function N() { return function () { return 1 } }
const X: any = N
const f = X()
console.log(typeof f, f())
function A() { return () => 2 }
const Y: any = A
console.log(Y()())
function K(k: number) { return (y: number) => k + y }
const Z: any = K
console.log(Z(2)(3), [1, 2].map(Z(10)).join(","))
function T(this: any) { return function () { return new.target === undefined } }
const W: any = T
const made = new W()
console.log(typeof made, made())
console.log(N()(), A()(), K(1)(1))
