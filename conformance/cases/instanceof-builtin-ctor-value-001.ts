// §7.3.22 OrdinaryHasInstance reads C's `.prototype`. A builtin
// constructor reached as a VALUE is a closure cell whose prototype is
// the registry singleton, not a lazily minted twin — asking for the
// twin threw "Function has non-object prototype" for every builtin
// whose constructor is not a real class.
const O: any = Object
const A: any = Array
const M: any = Map
const S: any = Set
const D: any = Date
const R: any = RegExp
const F: any = Function
const E: any = Error

console.log(({} as any) instanceof O, ([] as any) instanceof O)
console.log(([] as any) instanceof A, ({} as any) instanceof A)
console.log((new Map() as any) instanceof M, (new Set() as any) instanceof M)
console.log((new Set() as any) instanceof S)
console.log((new Date() as any) instanceof D, ({} as any) instanceof D)
console.log((/a/ as any) instanceof R)
console.log(((() => 1) as any) instanceof F, ({} as any) instanceof F)
console.log((new Error("x") as any) instanceof E)

// The prototype it reaches is the one everyone else sees.
console.log(O.prototype === Object.prototype, M.prototype === Map.prototype)

// A real class through the same value channel keeps its answer.
class C {}
const K: any = C
console.log(new C() instanceof K, ({} as any) instanceof K)
