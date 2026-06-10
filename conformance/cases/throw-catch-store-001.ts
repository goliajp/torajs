// throw a string param, store the catch binding (typed Any) into a
// Str-slotted top-level let — the assign site must unbox the Any
// rather than panic with a slot/value mismatch.

function risky(msg: string): number {
  if (msg.length > 3) {
    throw msg
  }
  return msg.length
}

let saved = ""
try {
  risky("abcdef")
} catch (e: any) {
  saved = e
}
console.log(saved)
console.log(saved.length)

let count = 0
try {
  risky("xyzzy!")
} catch (e: any) {
  count = count + 1
  saved = e
}
console.log(saved)
console.log(count)
