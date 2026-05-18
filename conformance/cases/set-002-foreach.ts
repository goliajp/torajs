// P6.4a — Set.forEach. Callback shape `(value, value2, set) => void`
// per spec §24.2.3.6 (the first two args are the same element).
// Same pattern as map-003 — print typeof each element inside the
// callback to verify the iteration without needing mutable capture.

class Box {
  v: number
  constructor(n: number) {
    this.v = n
  }
}

let s: Set = new Set()
let b1 = new Box(7)

s.add('hello')
s.add('world')
s.add(99)
s.add(b1)

console.log(s.size)

s.forEach((value: any, value2: any, set: Set) => {
  console.log(typeof value)
})

// Empty Set: callback never fires.
let empty: Set = new Set()
console.log('before-empty')
empty.forEach((v: any, v2: any, ss: Set) => {
  console.log('should-not-fire')
})
console.log('after-empty')

console.log(s.size)
