// Closing an inferred array-yielding generator: through two nested
// finally blocks, and with no finally at all (break, destructuring).
function* nested() {
  try {
    try {
      yield [1, 2]
      yield [3, 4]
    } finally {
      console.log("inner finally")
    }
  } finally {
    console.log("outer finally")
  }
  yield [5]
}
for (const v of nested()) {
  console.log("got", JSON.stringify(v))
  break
}
for (const v of nested()) {
  console.log("all", JSON.stringify(v))
}

function* plain() {
  yield [10]
  yield [20, 21]
  yield [30]
}
for (const v of plain()) {
  console.log("plain", JSON.stringify(v))
  if (v[0] === 20) break
}
const [a, b] = plain()
console.log("destr", JSON.stringify(a), JSON.stringify(b))
const [c] = nested()
console.log("destr nested", JSON.stringify(c))

function firstLen() {
  for (const v of plain()) return v.length
  return -1
}
console.log("return out", firstLen())
const it = nested()
console.log(JSON.stringify(it.next()))
console.log(JSON.stringify(it.next()))
console.log(JSON.stringify(it.next()))
console.log(JSON.stringify(it.next()))
console.log(JSON.stringify(it.next()))

function* typedNested(): Generator<number[]> {
  try {
    try {
      yield [7]
    } finally {
      console.log("typed inner")
    }
  } finally {
    console.log("typed outer")
  }
}
for (const v of typedNested()) {
  console.log("typed", JSON.stringify(v))
  break
}
console.log("end")
