// shift on typed arrays: the head bump, the length drop, the slot's
// value and stake, and the receivers that leave the plain bump (holes,
// locks, empty)
const q: number[] = []
let total = 0
for (let i = 0; i < 200; i = i + 1) {
  q.push(i * 1.5)
  if (q.length > 17) {
    total = total + q.shift()!
  }
}
console.log(total, q.length, q[0], q[q.length - 1])
while (q.length > 0) {
  total = total + q.shift()!
}
console.log(total, q.length, q.shift())

const ints: number[] = [7, 8, 9]
console.log(ints.shift(), ints.length, ints[0], ints.indexOf(9))
ints.push(10)
console.log(ints.join(","), ints.shift(), ints.join(","))

const words: string[] = []
for (let i = 0; i < 40; i = i + 1) {
  words.push("w" + i)
  if (words.length > 5) {
    const w = words.shift()!
    if (i % 10 === 0) console.log(w, w.length)
  }
}
console.log(words.join(" "))

const holey: number[] = [1, 2, 3]
holey.length = 5
holey[4] = 5
console.log(holey.shift(), holey.length, 2 in holey, 3 in holey, holey[3])
console.log(holey.shift(), holey.shift(), holey.shift(), holey.shift(), holey.length)

const sealed: number[] = [3, 1, 2]
Object.seal(sealed)
try {
  sealed.shift()
} catch (e) {
  console.log((e as Error).name, sealed.join(","))
}
const frozen: number[] = [4, 5]
Object.freeze(frozen)
try {
  frozen.shift()
} catch (e) {
  console.log((e as Error).name, frozen.length)
}

class Box {
  items: number[] = []
}
const b = new Box()
b.items.push(1, 2, 3)
console.log(b.items.shift(), b.items.shift(), b.items.length)
