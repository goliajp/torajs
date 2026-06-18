// Nested generic type args `Foo<Bar<X>>` per TS spec — the closing
// `>>` lexes as one ShrShr token; parser peels it into two virtual
// `>`s so the outer caller closes its own type-arg list. Same scheme
// covers `>>>` for `A<B<C<X>>>`.

// case A: 2-deep — Array<Array<number>>
const a: Array<Array<number>> = [[1, 2], [3, 4]]
console.log(a.length)
console.log(a[0].length)
console.log(a[1][1])

// case B: 3-deep — Array<Array<Array<number>>>
const b: Array<Array<Array<number>>> = [[[7]]]
console.log(b[0][0][0])

// case C: Map<string, Array<number>> with .size
const m: Map<string, Array<number>> = new Map<string, Array<number>>()
m.set("xs", [10, 20, 30])
console.log(m.size)

// case D: type alias with nested generic
type Lookup = Map<string, Array<number>>
const t: Lookup = new Map<string, Array<number>>()
t.set("a", [1])
console.log(t.size)

// case E: empty 2-deep (regression — no real ShrShr in `<>`)
const e: Array<Array<number>> = []
console.log(e.length)

// case F: function return type with nested generic
function makeArr(): Array<Array<number>> {
  return [[1], [2, 3]]
}
console.log(makeArr().length)
