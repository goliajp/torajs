// A `number` / `string` / `boolean` binding initialized with a read of
// an `any` value holds that value as is: the annotation has no runtime
// meaning and §14.3.1.2 / §9.1.1.1.5 bind without converting. So the
// binding is whatever the `any` held, `typeof` reads it at run time,
// and a number that really is one still computes as a number.

const str: any = 'str'
const zero: any = 0
const seven: any = '7'
const half: any = 2.5

const n: number = str
console.log(n, typeof n)

const s: string = zero
console.log(s, typeof s)

const b: boolean = str
console.log(b, typeof b)

const k: number = seven
console.log(k + 1, typeof k)

let m: number = seven
console.log(m + 1, typeof m)
m = 5
console.log(m + 1, typeof m)

// the value really is a number: arithmetic and methods as before
const q: number = half
console.log(q + 1, q * 2, q < 3, typeof q, q.toFixed(2))

let acc: number = half
for (let i = 0; i < 4; i++) acc = acc + i
console.log(acc, typeof acc)

try {
  console.log(n.toFixed(1))
} catch (e) {
  console.log((e as Error).name)
}

// a property, an element and a conditional read the same way
const obj: any = { a: 'A', b: 2 }
const sa: string = obj.a
const nb: number = obj.b
console.log(sa.length, nb * 3)
const bo: boolean = obj.missing
console.log(bo, typeof bo, bo ? 1 : 2)
const arr: any = [1, 'a']
const e: number = arr[1]
console.log(e, typeof e)
const cond: any = true
const t: string = cond ? str : zero
console.log(t, typeof t)
