// A thousand matches under `g` through a three-piece chain, and the
// chain over Latin-1 and UTF-16 subjects, whose pieces widen the
// result as they land.

let subject = ""
for (let i = 0; i < 1000; i = i + 1) {
  subject = subject + "k" + (i % 10) + "=" + i + ";"
}
const out = subject.replace(/([a-z]\d)=(\d+)/g, (m: string, k: string, v: string) => v + ":" + k + ",")
let sum = 0
for (let i = 0; i < out.length; i = i + 1) {
  sum = sum + out.charCodeAt(i)
}
console.log(out.length, sum, out.slice(0, 40), out.slice(out.length - 20))

const pair = /([^=;]+)=([^;]+)/g
console.log("é=1;ü=22;ß=333;".replace(pair, (m: string, k: string, v: string) => v + ":" + k + "|"))
console.log("中=1;文=22;字=333;".replace(pair, (m: string, k: string, v: string) => v + ":" + k + "|"))
console.log("a=中;b=é;c=x;".replace(pair, (m: string, k: string, v: string) => k + "→" + v + "·" + m))
console.log("𝄞=1;b=𝄞;".replace(pair, (m: string, k: string, v: string) => "[" + v + "]" + k + m.length))
// an ASCII result that a later piece widens
console.log("a=1;b=2;".replace(pair, (m: string, k: string, v: string) => k + v + "é"))
