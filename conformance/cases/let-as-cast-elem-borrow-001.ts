// A loop-body binding initialised by a cast or a non-null assertion
// over an element read borrows the element, as the uncast read does:
// releasing it at the end of each iteration freed the records the
// array still held.
type Rec = { key: string; n: number }
const recs: (Rec | undefined)[] = [{ key: 'a', n: 1 }, undefined, { key: 'c', n: 3 }]

let sum = 0
for (let i = 0; i < 3; i = i + 1) {
  try {
    const r = recs[i] as Rec
    sum = sum + r.n
  } catch (e) {
    console.log('first', i, e instanceof TypeError)
  }
}
console.log(sum)
for (let i = 0; i < 3; i = i + 1) {
  const q = recs[i] as Rec
  if (q === undefined) {
    console.log('second undefined', i)
    continue
  }
  console.log('second', i, q.n, q.key)
}
for (let i = 0; i < 3; i = i + 2) {
  const r = recs[i]!
  console.log('bang', r.key)
}
for (let i = 0; i < 3; i = i + 2) {
  const r = <Rec>recs[i]
  console.log('angle', r.n)
}
console.log(JSON.stringify(recs))

// the same through an object field and through a typed array
const box: { cur: Rec | undefined } = { cur: { key: 'b', n: 2 } }
for (let i = 0; i < 2; i = i + 1) {
  const r = box.cur as Rec
  console.log('field', r.key, r.n)
}
const typed: Rec[] = [{ key: 't', n: 7 }]
for (let i = 0; i < 2; i = i + 1) {
  const r = typed[0] as Rec
  console.log('typed', r.key, r.n)
}

// a cast that converts makes a value the binding owns
const mixed: any[] = ['x', 'yy', 'zzz']
for (let i = 0; i < 3; i = i + 1) {
  const s = mixed[i] as string
  console.log('str', s, typeof s)
}
const names: string[] = ['p', 'q']
for (let i = 0; i < 2; i = i + 1) {
  const s = names[i] as string
  console.log('name', s)
}
console.log(names.join(','), mixed.length)
