// A replace callback that only joins its parameters runs on views of
// the subject: the bench shape, global and non-global, with the
// result read afterwards.

const re = /([a-z]+)=(\d+)/g
const lines = ["ab=1;cd=22;=3;ef4;", "x=9;", "", "no pairs here", "k=0;k=00;"]
let total = 0
for (let i = 0; i < lines.length; i = i + 1) {
  const r = lines[i].replace(re, (whole: string, k: string, v: string) => v + ":" + k)
  console.log(r)
  total = total + r.length
}
console.log(total)

// non-global: one replacement, the rest of the subject untouched
console.log("a=1,b=2".replace(/(\w)=(\d)/, (m: string, k: string, v: string) => "[" + v + k + "]"))
// a top-level const regex binding names the same pattern
const swap = /(\w+) (\w+)/
console.log("Jane Doe".replace(swap, (m: string, a: string, b: string) => b + ", " + a))
// the match alone
console.log("a1b22c333".replace(/\d+/g, (m: string) => "<" + m + ">"))
// a constant replacer
console.log("x-y-z".replace(/-/g, () => "+"))
// replaceAll with the same callback shape
console.log("k1=v1,k2=v2".replaceAll(/(\w+)=(\w+)/g, (m: string, k: string, v: string) => k + ":" + v))
// no match: the subject comes back whole
console.log("nothing".replace(/\d/g, (m: string) => "#"))
