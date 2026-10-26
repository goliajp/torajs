// A slot whose object type has an optional member (`tag?`) besides a
// class-typed field, handed an object through `any`: the slot holds the
// object as it is, whether or not it owns a property under a method
// name of the class, as without the optional member.
class W {
  a = 1
  m(): number {
    return 10
  }
}
type O = { w: W; tag?: string }
function giveO(v: any): O {
  return v
}
const own: any = { a: 2, m() { return 20 } }
const other: any = { a: 5 }
try {
  const o = giveO({ w: own })
  console.log(o.w.a)
} catch (e) {
  console.log(e instanceof TypeError, (e as Error).message)
}
const o2 = giveO({ w: other, tag: "t" })
console.log(o2.w.a, o2.tag)
const o3 = giveO({ w: other })
console.log(o3.w.a, o3.tag, "tag" in o3)
