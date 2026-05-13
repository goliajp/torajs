// V3-18 wedge — bare type alias used as a class field type:
//   type N = number
//   class C { v: N = 42 }
// Per TS spec the alias resolves to its underlying primitive
// at the field-type level. Pre-fix tora's class factory built
// the field's default init by treating the bare alias's
// internal '__alias__' sentinel as a real struct field,
// generating 'Struct([("__alias__", Number(0))])' for what
// should be a plain 'Number(0)' default — the synthesized
// __this then failed type assignment with 'declared Number,
// init has Struct([("__alias__", Number)])'.
//
// Implementation: in ast::default_init_for_field, when the
// field type resolves to an alias_layouts entry whose only
// field is named '__alias__', recurse on the underlying ann
// instead of treating it as a struct shape.

type N = number
type S = string
type B = boolean

class C {
  v: N = 42
  s: S = "hi"
  b: B = true
  describe(): string {
    return "v=" + this.v + " s=" + this.s + " b=" + this.b
  }
}
let c = new C()
console.log(c.describe())              // v=42 s=hi b=true
console.log(c.v, c.s, c.b)             // 42 hi true

// Alias of array-of-T as a field.
type IntArr = number[]
class Bag {
  xs: IntArr = [1, 2, 3]
  total(): number {
    let t = 0
    for (let x of this.xs) t += x
    return t
  }
}
let bag = new Bag()
console.log(bag.total())               // 6
console.log(bag.xs.length)             // 3

// Alias of nullable as a field.
type MaybeStr = string | null
class Box {
  label: MaybeStr = null
}
let box = new Box()
console.log(box.label)                 // null
box.label = "set"
console.log(box.label)                 // set
