// Redefining one declared field marks the whole instance exotic, but a
// field no definer names stays a plain slot: its typed store skips the
// guard, its typed read is a bare load, and both still agree with the
// any lane and with the redefined field's own refusal.
class Box {
  n: number
  tag: string
  constructor(n: number) {
    this.n = n
    this.tag = "t"
  }
}

const b = new Box(3)
Object.defineProperty(b, "tag", { value: "x", writable: false })
b.n = 5
const bb: any = b
console.log(b.n, bb.n, b.tag, JSON.stringify(b))
try {
  b.tag = "y"
  console.log("no throw", b.tag)
} catch (e) {
  console.log("caught", (e as Error).constructor.name, b.tag)
}
b.n = b.n + 1
console.log(b.n, Object.keys(b).join(","))
