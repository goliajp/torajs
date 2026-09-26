// A mutable binding inferred from its initializer, later assigned a
// value typed `any`, holds that value as is (§13.15.2 PutValue does
// not convert): the binding widens to `any` from its declaration.

const s: any = 'x'
const n: any = 5

let total = 0
total = total + s
console.log(total, typeof total)

let count = 0
count = count + n
console.log(count, typeof count)

let m = 0
m = s
console.log(m, typeof m)

let u = 1
u = total
console.log(u, typeof u)

let flag = true
flag = s
console.log(flag, typeof flag)

let acc = 0.5
const items: any[] = [1, 'b', 2.5, null, true]
for (const it of items) acc = acc + it
console.log(acc, typeof acc)

// typed accumulation stays numeric
let i = 0
for (let k = 0; k < 5; k = k + 1) i = i + k
console.log(i, typeof i)

let str = ''
str = str + s
console.log(str, typeof str)

// a binding read by a named function (a global slot)
let shared = 0
function readShared(): any {
  return shared
}
shared = shared + s
console.log(readShared(), typeof readShared())
