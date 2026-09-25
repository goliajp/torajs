// `c ? new C() : null` into a `C | null` binding: the null arm is a
// null reference, so the null test holds and the other arm reads its
// fields (the arm used to arrive boxed and was stored as box bits)

class P {
  x: number
  constructor(x: number) {
    this.x = x
  }
}

let sum = 0
let nulls = 0
for (let i = 0; i < 6; i = i + 1) {
  const p: P | null = i % 3 === 0 ? new P(i) : null
  if (p === null) {
    nulls = nulls + 1
  } else {
    sum = sum + p.x
  }
}
console.log(sum, nulls)

function pick(flag: boolean): P | null {
  return flag ? null : new P(7)
}
const a = pick(true)
const b = pick(false)
console.log(a === null, b !== null ? b.x : -1)

let q: P | null = null
q = sum > 1 ? new P(sum) : null
console.log(q === null ? "none" : q.x)
