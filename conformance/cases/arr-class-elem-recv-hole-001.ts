// A hole in a class-typed array, read and then member-accessed: the
// hole reads as `undefined` (§10.4.2.1 — no own element), and the
// field read on it is a catchable TypeError with the same wording an
// out-of-range read gets. A live element beside the hole still reads
// its field.

class Node {
  id: number
  next: Node | null
  constructor(id: number) {
    this.id = id
    this.next = null
  }
}

const xs: Node[] = [new Node(1)]
xs[3] = new Node(4)
console.log(xs.length)
console.log(xs[0].id, xs[3].id)
console.log(xs[1] === undefined, xs[2] === undefined)

function idAt(i: number): string {
  try {
    return "id=" + xs[i].id
  } catch (e: any) {
    const m: string = e.message as string
    return (e instanceof TypeError) + ":" + (m.startsWith("undefined is not an object") ? "undefined" : m)
  }
}
console.log(idAt(0))
console.log(idAt(1))
console.log(idAt(2))
console.log(idAt(3))
console.log(idAt(4))
console.log(idAt(-1))

// the receiver is a method's own field: same lane, same answers
class Holder {
  items: Node[]
  constructor(items: Node[]) {
    this.items = items
  }
  idOf(i: number): number {
    return this.items[i].id
  }
  idOfSafe(i: number): string {
    try {
      return "id=" + this.items[i].id
    } catch (e: any) {
      return e instanceof TypeError ? "TypeError" : "other"
    }
  }
}
const h = new Holder(xs)
console.log(h.idOf(3), h.idOf(0))
let caught = 0
for (let i = 0; i < 6; i = i + 1) {
  if (h.idOfSafe(i) === "TypeError") caught = caught + 1
}
console.log(caught)
