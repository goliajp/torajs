// A container that an element of an escaped typed array is stored
// into holds it as it is: an array literal, `push`, `unshift`, an
// index store, an object literal field, and an array written by a
// function value called through `any`.

const d: number[] = [1, 2, 3]
const x: any = d
x[1] = "s"

const lit = [d[1], 5]
console.log(JSON.stringify(lit), typeof lit[0])

const acc: number[] = []
acc.push(d[1])
acc.push(d[0])
acc.unshift(d[1])
console.log(JSON.stringify(acc), acc.length)

const ys: number[] = [0, 0]
ys[0] = d[1]
let sum = 0
for (const y of ys) {
  sum = sum + y
}
console.log(ys[0], typeof ys[0], sum)

const box = { v: d[1], w: 2 }
let f = 0
f = f + box.v
console.log(f, box.w + 1)

const sink: number[] = []
const put: any = (v: number) => {
  sink.push(v)
}
put("t")
put(4)
console.log(JSON.stringify(sink))

let at = 0
at = at + d.at(1)!
console.log(at)

const grid: number[][] = [[0], [1]]
grid[1].push(d[1])
console.log(JSON.stringify(grid))

// arrays that never hold such a value stay typed
const t: number[] = [1, 2]
t.push(3)
const u = [t[0], t[2]]
let ts = 0
for (const v of u) {
  ts = ts + v
}
console.log(ts, typeof u[0])
