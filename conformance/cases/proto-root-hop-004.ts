// The three faces the inherited-surface fallback serves are
// themselves %Object.prototype% entries, so a program's write to the
// same name replaces them (521-06). Before the consult was ordered
// ahead of them, a patched `Object.prototype.toString` lost to the
// "[object Object]" badge for every plain object.
;(Object.prototype as any).toString = function () {
  return "PATCHED"
}
;(Object.prototype as any).valueOf = function () {
  return 42
}

const o: any = { x: 1 }
console.log(o.toString())
console.log(String(o))
console.log(`${o}`)
console.log(o.valueOf())

class C {
  x = 1
}
console.log(String(new C()))

// An OWN entry still shadows the patched prototype face.
const own: any = {
  toString() {
    return "own"
  },
}
console.log(String(own), own.toLocaleString())

// Other lanes reached the same patch from the dispatcher tail
// already; they must keep answering it.
console.log(String([1, 2]) === "1,2", [1, 2].toString())
