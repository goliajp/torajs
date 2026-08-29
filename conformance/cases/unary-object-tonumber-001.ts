// §7.1.4 ToNumber has an answer for every object shape — its own
// OrdinaryToPrimitive, reaching `valueOf` then `toString`. The
// checker used to reject `+xs` on a `number[]` as an unsupported
// operand while `+({} as any)` compiled and did the same coercion.
const xs: number[] = [1]
const ys: number[] = [1, 2]
const zs: number[] = []
console.log(+xs, +ys, +zs, -xs, -zs)
// §13.7 ToNumeric runs on each side of the binary lanes too, and the
// same list decides which shapes get there — `+xs` and `xs * 1` used
// to disagree about arrays.
console.log(xs * 1, ys * 1, zs * 3)
console.log(xs - 1, ys / 2, xs % 1)

const ss: string[] = ["3"]
console.log(+ss, +["4.5"])

const d = new Date(1234)
console.log(+d, -d)

const m = new Map<string, number>()
const s = new Set<number>()
console.log(+m, +s)

const r = /a/
console.log(+r)

const f = (n: number) => n
console.log(+f)

class C {
  valueOf() {
    return 7
  }
}
console.log(+new C(), -new C())

// A throwing valueOf propagates out of the coercion.
class T {
  valueOf(): number {
    throw new RangeError("no")
  }
}
try {
  console.log(+new T())
} catch (e) {
  console.log("threw", (e as Error).constructor.name)
}

const d2 = new Date(10)
console.log(d2 * 2, d2 - 5)
console.log([2] ** 3, [8] / [2])
