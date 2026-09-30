// A class instance handed from one owner to the next — a fresh
// temporary pushed into an array, a fresh node assigned over the
// binding it wraps — takes a reference for the new owner and gives up
// the old one. The two cancel; the instance keeps exactly the
// references its owners hold, is released once when the last owner
// goes, and a ring built from moved instances is still collected.
class Item {
  id: number
  link: Item | null
  constructor(id: number, link: Item | null) {
    this.id = id
    this.link = link
  }
}

function fill(n: number): Item[] {
  const out: Item[] = []
  for (let i = 0; i < n; i = i + 1) {
    out.push(new Item(i, null))
  }
  return out
}
const items = fill(5)
let ids = ""
for (let i = 0; i < items.length; i = i + 1) ids = ids + items[i].id
console.log("pushed:", ids, items.length)

function chain(n: number): Item {
  let head = new Item(0, null)
  for (let i = 1; i < n; i = i + 1) {
    head = new Item(i, head)
  }
  return head
}
function total(h: Item | null): number {
  let s = 0
  while (h !== null) {
    s = s + h.id
    h = h.link
  }
  return s
}
console.log("chain:", total(chain(10)))

// the same instance kept by a second owner across the move
function shared(): number {
  const out: Item[] = []
  const keep = new Item(7, null)
  out.push(keep)
  let head = keep
  head = new Item(8, head)
  out.pop()
  return keep.id + total(head) + out.length
}
console.log("shared:", shared())

// a null moved the same way
function empty(): number {
  let head: Item | null = null
  head = new Item(1, head)
  return total(head)
}
console.log("empty:", empty())

// rings made of moved instances, dropped every round
let live = 0
for (let r = 0; r < 50000; r = r + 1) {
  const ring: Item[] = []
  ring.push(new Item(r, null))
  ring.push(new Item(r + 1, ring[0]))
  ring[0].link = ring[1]
  let head = ring[1]
  head = new Item(r + 2, head)
  live = (live + head.id + total(chain(3))) | 0
}
console.log("rings:", live)
