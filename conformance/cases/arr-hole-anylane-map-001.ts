// §23.1.3.21 step 6.b — map asks HasProperty before it calls
// anything, but unlike forEach and filter it cannot just step past an
// absent index: its product has the source's length, so the skipped
// position has to arrive in the destination as a hole of its own
// (step 6.c leaves it uncreated). The ANY lane's shared loop gated
// forEach and filter and left map calling the callback on every hole.
//
// A file reaches that lane just by naming `Object` as a VALUE.
const anchor: any = Object

let calls = 0
const elided = ([, 9] as any[]).map(function (v: any) {
  calls = calls + 1
  return 1
})
console.log("elided  :", calls, elided.length, JSON.stringify(elided))
console.log("elided in:", 0 in elided, 1 in elided)

const deleted: any[] = [1, 2, 3]
delete deleted[1]
calls = 0
const mapped = deleted.map(function (v: any) {
  calls = calls + 1
  return v * 10
})
console.log("deleted :", calls, mapped.length, JSON.stringify(mapped))
console.log("deleted in:", 0 in mapped, 1 in mapped, 2 in mapped)

// A length-grow gap is holes all the way; the product keeps them.
const grown: any[] = []
grown[3] = 1
calls = 0
const grownMapped = grown.map(function () {
  calls = calls + 1
  return 7
})
console.log("grown   :", calls, grownMapped.length, JSON.stringify(grownMapped))

// Dense arrays are untouched — every index is present.
calls = 0
const dense = ([1, 2, 3] as any[]).map(function (v: any) {
  calls = calls + 1
  return v + 1
})
console.log("dense   :", calls, JSON.stringify(dense))

// HasProperty does not stop at own: an index the prototype supplies
// makes the receiver's hole a property, and the callback runs there —
// the product gets a real element in that slot, not a hole.
Object.defineProperty(Array.prototype, "0", {
  get: function () {
    return 42
  },
  configurable: true,
})
calls = 0
const supplied = ([, 9] as any[]).map(function (v: any) {
  calls = calls + 1
  return v
})
console.log("supplied:", calls, JSON.stringify(supplied))
console.log("supplied in:", 0 in supplied, 1 in supplied)
