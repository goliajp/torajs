// A closure array whose elements have DIFFERENT signatures: the
// anchor decides the slot, so every indirect call site would be
// generated against the first element's native ABI. Reading a
// `(x) => boolean` body's raw i1 back as a NaN box dereferenced
// 0x5 (test262 staging/sm Iterator lazy-methods-reentry), and the
// number twin answered `boolean true` instead of `number 7`.
const fns = [(x: any) => x, (x: any) => true, (x: any) => 7]
for (const g of fns) {
  console.log(typeof g(1), g(1))
}

// The anchor's own position must not matter either.
const flipped = [(x: any) => true, (x: any) => x]
console.log(typeof flipped[1](5), flipped[1](5))

// Both disagreements survive nesting — the outer pair agrees
// (`Array<Function>` on both sides, neither `Any`) and only the
// inner call goes through the wrong ABI.
const nested = [[(x: any) => x], [(x: any) => true]]
for (const pair of nested) {
  console.log(typeof pair[0](2), pair[0](2))
}

// Same-signature elements keep the narrow slot and must still work.
const same = [(x: any) => x + 1, (x: any) => x + 2]
console.log(same[0](10), same[1](10))
