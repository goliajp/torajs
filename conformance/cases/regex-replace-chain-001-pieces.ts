// A replace callback whose body is a concat chain writes its pieces
// straight onto the result: two, three and five pieces, views and
// literals in every position, global and not.

const re = /([a-z]+)=(\d+)/g
const line = "ab=1;cd=22;=3;ef4;gh=555;"
console.log(line.replace(re, (m: string, k: string, v: string) => v + ":" + k))
console.log(line.replace(re, (m: string, k: string, v: string) => v + ":" + k + ";"))
console.log(line.replace(re, (m: string, k: string, v: string) => "<" + v + ":" + k + ">"))
console.log(line.replace(re, (m: string, k: string, v: string) => k + v + m + k + v))
console.log(line.replace(re, (m: string, k: string, v: string) => "[" + m + "]" + "(" + k + ")" + v))
// the literal first, the view last, and a view twice
console.log(line.replace(re, (m: string, k: string, v: string) => "=" + v))
console.log(line.replace(re, (m: string, k: string, v: string) => v + v))
// the match only
console.log(line.replace(/\d+/g, (m: string) => "<" + m + ">" + m))
// non-global
console.log(line.replace(/([a-z]+)=(\d+)/, (m: string, k: string, v: string) => v + ":" + k + "!"))
// the result grows past its first allocation
const long = line + line + line + line + line + line + line + line
const r = long.replace(re, (m: string, k: string, v: string) => v + "=" + k + "|" + m)
console.log(r.length, r)
// a number in the chain
console.log(line.replace(re, (m: string, k: string, v: string) => k + "#" + v.length + "#" + m.length))
