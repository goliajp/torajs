// a tag may annotate its template object as TemplateStringsArray
function tag(s: TemplateStringsArray, ...v: number[]): string {
  return s.raw.join("|") + ":" + s.join("/") + ":" + v.join("+") + ":" + s.length
}
console.log(tag`a${1}b\n${2}c`)
function first(s: TemplateStringsArray): string { return s[0] }
console.log(first`hello${0}`)
const same = (s: TemplateStringsArray) => s
const f = () => same`x`
console.log(f() === f(), Object.isFrozen(f()))
