// throw a string param, store the catch binding (typed Any) into a
// top-level let initialized with a string: the binding takes the
// thrown value as is (it widens to any), and reads of it see a string.

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
