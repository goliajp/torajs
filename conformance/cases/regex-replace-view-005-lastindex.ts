// What a replace callback can observe of lastIndex, and what a
// sticky pattern does with it. §22.2.6.11 collects every match before
// the first callback runs: under `g` the callback sees 0 and cannot
// change the match set; without `g` a sticky pattern is one exec at
// lastIndex and stores the end (or 0 on a miss).

const g = /a/g
g.lastIndex = 3
console.log("banana".replace(g, (m: string) => m + g.lastIndex))
console.log(g.lastIndex)
console.log(
  "banana".replace(g, (m: string) => {
    g.lastIndex = 5
    return "A"
  }),
)
console.log(g.lastIndex)

const y = /a/y
y.lastIndex = 1
console.log("baa".replace(y, (m: string) => "[" + m + "]"), y.lastIndex)
y.lastIndex = 0
console.log("baa".replace(y, (m: string) => "[" + m + "]"), y.lastIndex)
y.lastIndex = 9
console.log("baa".replace(y, (m: string) => "[" + m + "]"), y.lastIndex)

const gy = /a/gy
gy.lastIndex = 2
console.log("aaXa".replace(gy, (m: string) => "b"), gy.lastIndex)

// the same sticky walk with a callback the view lane does not take
// (its parameter is handed to a method): the kernel's own answer
y.lastIndex = 1
console.log("baa".replace(y, (m: string) => m.toUpperCase()), y.lastIndex)
y.lastIndex = 0
console.log("baa".replace(y, (m: string) => m.toUpperCase()), y.lastIndex)
y.lastIndex = 9
console.log("baa".replace(y, (m: string) => m.toUpperCase()), y.lastIndex)

// a plain pattern never touches it
const p = /a/
p.lastIndex = 2
console.log("baa".replace(p, (m: string) => "[" + m + "]"), p.lastIndex)

// a read-only lastIndex makes the global replace's store throw
const frozen = /a/g
Object.defineProperty(frozen, "lastIndex", { writable: false })
try {
  console.log("aXa".replace(frozen, (m: string) => "b"))
} catch (e) {
  console.log("caught", e instanceof TypeError)
}
const stickyFrozen = /a/y
Object.defineProperty(stickyFrozen, "lastIndex", { writable: false })
try {
  console.log("aXa".replace(stickyFrozen, (m: string) => "b"))
} catch (e) {
  console.log("caught", e instanceof TypeError)
}
