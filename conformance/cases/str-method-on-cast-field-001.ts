// A string method called on a field read off an `as`-cast any value:
// the checker types the receiver a string, the read lowers to an any
// value, and the call runs through the runtime dispatcher

function head(e: any): string {
  return (e as Error).message.slice(0, 3)
}
console.log(head(new Error("hello")))

let caught: any = null
try {
  throw new TypeError("boom! bang")
} catch (e) {
  caught = e
}
const err = caught as Error
console.log(err.message.toUpperCase(), err.name.length)
console.log(err.message.split(" ").length, err.message.indexOf("bang"), err.message.includes("!"))
console.log(err.message.replace("boom", "B").padEnd(12, "."), err.message.at(-1))

type Rec = { label: string; n: number }
const raw: any = JSON.parse('{"label":"  spaced  ","n":2}')
const rec = raw as Rec
console.log("[" + rec.label.trim() + "]", rec.label.repeat(rec.n).length)
