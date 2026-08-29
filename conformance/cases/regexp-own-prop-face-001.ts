// §22.2.6 — a RegExp instance owns `lastIndex` in the cell itself and
// everything else in the ordinary own face. Writing a name key must
// not disturb the compiled program, and `lastIndex` keeps its own
// non-enumerable slot.
const r: any = /a(b)/g
r.label = "ab"
console.log(r.label, r.source, r.flags, r.lastIndex)
console.log(Object.getOwnPropertyNames(r), Object.keys(r))
console.log(JSON.stringify("xab".match(/a(b)/)))
r.lastIndex = 2
console.log(r.lastIndex, r.label)
console.log(JSON.stringify(Object.getOwnPropertyDescriptor(r, "label")))
delete r.label
console.log(r.label, "label" in r, r.lastIndex)
