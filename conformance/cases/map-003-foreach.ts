// P6.4a — Map.forEach. Callback shape `(value, key, map) => void`
// per spec §23.1.3.6. We print key tags inside the callback body
// (avoiding mutable closure captures, which need the P6+ closure-
// capture substrate not yet shipped). Each entry's typeof-key
// signature is observable + the size accessor lets us cross-check
// the loop visited every live entry.

class Box {
  v: number
  constructor(n: number) {
    this.v = n
  }
}

let m: Map = new Map()
let k1 = new Box(10)

m.set('alpha', 1)
m.set('beta', 2)
m.set(42, 'forty-two')
m.set(k1, 'box-val')

console.log(m.size)

// Each callback emits one line per entry — typeof key tells us
// which side of the key domain we hit (string / number / object).
m.forEach((value: any, key: any, map: Map) => {
  console.log(typeof key)
})

// Empty Map: forEach must not invoke the callback (no output
// between the two markers).
let empty: Map = new Map()
console.log('before-empty')
empty.forEach((v: any, k: any, mm: Map) => {
  console.log('should-not-fire')
})
console.log('after-empty')

// Final size unchanged by forEach.
console.log(m.size)
