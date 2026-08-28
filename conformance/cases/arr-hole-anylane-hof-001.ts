// §23.1.3.15 step 4.b / §23.1.3.21 step 6.b — forEach and filter ask
// HasProperty before they call anything, so a hole nothing on the
// chain supplies is skipped. The ANY lane's shared loop had no such
// gate: it read every index and called the callback on it.
//
// The reason this stayed hidden is worth writing down: a file reaches
// that lane just by naming `Object` or `Array` as a VALUE, which is
// ordinary enough that the sibling fixture arr-hole-proto-has-001 was
// green only because an AST rewrite happened to erase its
// `Object.prototype` mention.
const anchor: any = Object

const elided: any[] = [, 9]
let seen: string = ""
elided.forEach(function (v: any, i: number) {
  seen = seen + String(i) + " "
})
console.log("elided  :", seen)

const deleted: any[] = [1, 2, 3]
delete deleted[1]
seen = ""
deleted.forEach(function (v: any, i: number) {
  seen = seen + String(i) + " "
})
console.log("deleted :", seen)

// A length-grow gap is holes all the way, and none of them are
// visited.
const grown: any[] = []
grown[5] = 1
seen = ""
grown.forEach(function (v: any, i: number) {
  seen = seen + String(i) + " "
})
console.log("grown   :", "[" + seen + "]", grown.length)

// filter drops the same indices without asking the callback.
console.log("filter  :", JSON.stringify([, 9, , 3].filter(function () { return true })))

// Dense arrays are untouched — every index is present.
seen = ""
;[1, 2, 3].forEach(function (v: any, i: number) {
  seen = seen + String(i) + ":" + String(v) + " "
})
console.log("dense   :", seen)
console.log("filter2 :", JSON.stringify([1, 2, 3, 4].filter(function (v: any) { return v % 2 === 0 })))

// HasProperty does not stop at own: an index the prototype supplies
// makes the receiver's hole a property, and the callback runs there.
Object.defineProperty(Array.prototype, "0", {
  get: function () {
    return 42
  },
  configurable: true,
})
seen = ""
;([, 9] as any[]).forEach(function (v: any, i: number) {
  seen = seen + String(i) + ":" + String(v) + " "
})
console.log("supplied:", seen)
