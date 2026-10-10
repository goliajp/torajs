// Values that reach a slot typed as a class without being an instance
// of it: an object literal of the class's shape (the checker accepts
// one), directly, through a struct-typed binding, inside an array
// literal and as a return value, and a cast. Each literal hands the
// field an element read past the end, so the field holds undefined
// and the next member read throws (§13.3.2.1 step 3 via ToObject).
class Leaf {
  v: number
  constructor(v: number) {
    this.v = v
  }
}
class Box {
  item: Leaf
  constructor(item: Leaf) {
    this.item = item
  }
}
function name(e: unknown): string {
  return e instanceof TypeError ? "TypeError" : "other"
}
const xs: Leaf[] = []
const lit: Box = { item: xs[0] }
try {
  console.log(lit.item.v)
} catch (e) {
  console.log("literal:", name(e))
}
const s = { item: xs[0] }
const viaStruct: Box = s
try {
  console.log(viaStruct.item.v)
} catch (e) {
  console.log("struct binding:", name(e))
}
const list: Box[] = [{ item: xs[0] }]
try {
  console.log(list[0].item.v)
} catch (e) {
  console.log("array element:", name(e))
}
function give(): Box {
  return { item: xs[0] }
}
try {
  console.log(give().item.v)
} catch (e) {
  console.log("return:", name(e))
}
const empty = {} as Box
try {
  console.log(empty.item.v)
} catch (e) {
  console.log("cast:", name(e))
}
console.log(new Box(new Leaf(2)).item.v)
