// A chain with a throwing call in its middle ends the walk where the
// throw lands, with the pieces built before it released; a chain
// whose result is also kept alive answers the same as one that is
// only appended.

let ticks = 0
function tick(): string {
  ticks = ticks + 1
  if (ticks === 3) throw new Error("tick " + ticks)
  return "#" + ticks
}
try {
  const r = "a=1;b=2;c=3;d=4;".replace(/([a-z])=(\d)/g, (m: string, k: string, v: string) => v + ":" + tick() + k + "!")
  console.log("unreachable", r)
} catch (e) {
  console.log("caught", (e as Error).message, ticks)
}
ticks = 0
console.log("a=1;b=2;".replace(/([a-z])=(\d)/g, (m: string, k: string, v: string) => v + ":" + tick() + k + "!"))

// the intermediate string is stored as well as returned
const kept: string[] = []
console.log(
  "a=1;b=2;c=3;".replace(/([a-z])=(\d)/g, (m: string, k: string, v: string) => {
    const t = v + ":" + k
    kept.push(t)
    return t
  }),
)
console.log(kept.join("|"))

// the intermediate string is the last one seen
let last = ""
console.log(
  "a=1;b=2;c=3;".replace(/([a-z])=(\d)/g, (m: string, k: string, v: string) => {
    last = v + "-" + k
    return last
  }),
)
console.log(last)

// the callback's answer is read twice by its own body
console.log(
  "a=1;b=2;".replace(/([a-z])=(\d)/g, (m: string, k: string, v: string) => {
    const t = v + ":" + k
    return t + t.length
  }),
)
