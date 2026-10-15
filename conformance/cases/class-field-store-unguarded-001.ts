// A typed store into a declared class field skips the frozen /
// redefined-field test when no spelling in the program can freeze an
// object, redefine that field or park a value in its sidecar. This
// program spells none of them, so every store below is a plain slot
// write; the reads, the keys and the JSON form agree with it.
class Cursor {
  pos: number
  label: string
  items: number[]
  constructor(items: number[]) {
    this.pos = 0
    this.label = "start"
    this.items = items
  }
  step(): number {
    const v = this.items[this.pos]
    this.pos = this.pos + 1
    this.label = "at " + this.pos
    return v
  }
}

const c = new Cursor([4, 5, 6])
let sum = 0
while (c.pos < c.items.length) sum = sum + c.step()
c.pos++
c.items = [7]
console.log(sum, c.pos, c.label, c.items.length)
console.log(Object.keys(c).join(","), JSON.stringify(c))
