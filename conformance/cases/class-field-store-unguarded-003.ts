// A deleted declared field (§10.1.10) comes back as a plain own data
// property on the next typed store (§10.1.9.2 step 2.c), in a program
// that names no definer and never freezes: `delete` is the one
// spelling that touches the field, and the store must still see it.
class Box {
  n: number
  tag: string
  constructor(n: number) {
    this.n = n
    this.tag = "t"
  }
}

const b = new Box(3)
const bb: any = b
console.log(delete bb.tag, Object.keys(b).join(","), JSON.stringify(b))
b.tag = "back"
console.log(b.tag, Object.keys(b).join(","), JSON.stringify(b))
