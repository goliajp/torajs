// An object literal field that is stored a value of another kind than
// its initializer holds that value as it is: an element of a typed
// array whose alias escaped into the `any` world, and the argument of a
// function value called through `any`.

const d: number[] = [1, 2, 3]
const x: any = d
x[1] = "s"

const o = { k: 0 }
o.k = d[1]
console.log(o.k, typeof o.k)
let m = 0
m = m + o.k
console.log(m)

const obj = { k: 0 as number, n: 1 }
const h: any = (v: number) => {
  obj.k = v
}
h("x")
console.log(obj.k, typeof obj.k, obj.n + 1)
h(5)
console.log(obj.k * 2)

const label = { text: "a" }
const setText: any = (t: string) => {
  label.text = t
}
setText(7)
console.log(label.text, typeof label.text)

const box = { v: d[1] }
let f = 0
f = f + box.v
console.log(f, typeof box.v)

// an object literal no such value reaches keeps its typed fields
const p = { a: 1, b: 2 }
p.a = p.a + p.b
console.log(p.a, typeof p.a)
