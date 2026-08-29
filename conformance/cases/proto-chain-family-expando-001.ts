// §10.1.8.1 step 4 — the read walks the WHOLE prototype chain, not
// just the receiver's own family and not just the root. §23.1.5.2
// puts %ArrayIteratorPrototype% under %Iterator.prototype%, so a
// property installed there sits BETWEEN an array iterator and the
// chain root.
const IP: any = (Iterator as any).prototype
IP.zz = 9
const ai: any = [1].values()
const si: any = "ab"[Symbol.iterator]()
console.log(ai.zz, si.zz)

// One link down: %ArrayIteratorPrototype%'s own expando reaches an
// array iterator and NOT a map iterator — they are siblings, not
// ancestors.
const AIP: any = Object.getPrototypeOf([1].values())
AIP.qq = 3
const mi: any = new Map([[1, 2]]).entries()
console.log(ai.qq, mi.qq, mi.zz)

// Own beats family, family beats root.
const AP: any = (Array as any).prototype
AP.rr = "family"
;(Object.prototype as any).rr = "root"
const arr: any = [1]
console.log(arr.rr)
arr.rr = "own"
console.log(arr.rr)
const plain: any = {}
console.log(plain.rr)

// A spec-given method is still the method.
console.log([1, 2].map((x: number) => x + 1))
console.log(ai.nope, mi.nope)
delete (Object.prototype as any).rr
