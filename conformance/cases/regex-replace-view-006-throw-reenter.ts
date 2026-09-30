// A callback that throws ends the walk at that match and the throw is
// caught where the replace stands; a callback that runs the same
// regex again, or one whose parameter escapes, keeps working.

let calls = 0
try {
  const r = "a1b2c3".replace(/\d/g, (m: string) => {
    calls = calls + 1
    if (m === "2") throw new Error("stop at " + m)
    return "[" + m + "]"
  })
  console.log("unreachable", r)
} catch (e) {
  console.log("caught", (e as Error).message, calls)
}

// the same regex inside its own callback
const re = /\d/g
console.log(
  "1-2".replace(re, (m: string) => {
    const inner = "789".replace(re, (n: string) => "<" + n + ">")
    return m + inner
  }),
)
console.log(re.lastIndex)

// a view of the subject passed on: these shapes keep the kernel, and
// must answer the same
const kept: string[] = []
console.log(
  "x1y2".replace(/\d/g, (m: string) => {
    kept.push(m)
    return "_"
  }),
)
console.log(kept.join(","))
function shout(s: string): string {
  return s.toUpperCase()
}
console.log("ab".replace(/\w/g, (m: string) => shout(m)))
console.log("ab".replace(/\w/g, (m: string) => m.toUpperCase()))
const same = (m: string) => m + m
console.log("ab".replace(/\w/g, same))

// a subject that is itself a view
const line = "prefix|k=v;a=b"
const part = line.slice(7)
console.log(part.replace(/(\w)=(\w)/g, (m: string, k: string, v: string) => v + k))
