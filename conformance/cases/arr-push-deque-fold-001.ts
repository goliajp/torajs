// push into a shifted array: the head folds back to slot 0, and the
// buffer grows when the fold would leave it half full or more; queues
// that hover at, under and over half a buffer, a queue drained to
// empty and refilled, and a never-shifted array growing from empty
function run(keep: number, n: number): string {
  const q: number[] = []
  let sum = 0
  for (let i = 0; i < n; i = i + 1) {
    q.push(i)
    if (q.length > keep) {
      sum = sum + q.shift()! * (i % 7)
    }
  }
  return sum + " " + q.length + " " + q[0] + " " + q[q.length - 1] + " " + q.indexOf(n - 1)
}
for (const keep of [1, 7, 8, 9, 15, 16, 17, 31, 32, 33, 64]) {
  console.log(keep, run(keep, 1000))
}

const d: string[] = []
for (let round = 0; round < 4; round = round + 1) {
  for (let i = 0; i < 20; i = i + 1) d.push("r" + round + "i" + i)
  let s = ""
  while (d.length > 0) s = d.shift()!
  console.log(round, s, d.length)
}
d.push("last")
console.log(d.length, d[0], d.join("|"))

const grow: number[] = []
for (let i = 0; i < 100; i = i + 1) grow.push(i * 2)
console.log(grow.length, grow[0], grow[99], grow.reduce((a, b) => a + b, 0))

const mixed: number[] = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10]
mixed.shift()
mixed.shift()
mixed.shift()
for (let i = 0; i < 12; i = i + 1) mixed.push(100 + i)
console.log(mixed.length, mixed.join(","))
console.log(mixed.slice(2, 5).join(","), mixed.lastIndexOf(111), mixed.includes(3))
