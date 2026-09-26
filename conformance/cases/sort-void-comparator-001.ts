// a comparator that returns nothing compares every pair equal
// (ToNumber(undefined) is NaN, read as +0): the sort keeps the input
// order, still calls the comparator, and a throw from it stops the
// sort at the first call
let calls = 0
const a = [5, 3, 9, 1]
a.sort(() => {
  calls = calls + 1
})
console.log(a.join(","), calls > 0)

const w = ["pear", "fig", "apple"]
const seen: string[] = []
const t = w.toSorted((x, y) => {
  seen.push(x + "/" + y)
})
console.log(t.join(","), w.join(","), seen.length > 0)

let called = 0
try {
  ;[4, 2, 8].toSorted(() => {
    called = called + 1
    if (called === 1) throw new Error("stop")
  })
} catch (e) {
  console.log((e as Error).message, called)
}

let c2 = 0
const b = [2, 1]
b.sort((x: number, y: number) => {
  c2 = c2 + x + y
}, 0)
console.log(b.join(","), c2)
