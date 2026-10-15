// In the program's main body, a binding declared inside a block keeps
// the answer of its first nullish guard for the rest of the path, and
// loses it wherever it can change: a new iteration, a write of its own,
// a function that writes a `var`. Each read of an element that is
// missing still throws its TypeError.

type Rec = { key: string; n: number }
function mk(): Rec[] {
  const xs: Rec[] = []
  xs[0] = { key: 'a', n: 1 }
  xs[2] = { key: 'c', n: 3 }
  xs.length = 4
  return xs
}
const recs = mk()
const recsB = mk()
const recsD = mk()
const recsE = mk()

const seen: number[] = []
for (let i = 0; i < recs.length; i = i + 1) {
  try {
    const r = recs[i]
    seen.push(r.n)
    seen.push(r.key.length, r.n)
  } catch (e) {
    console.log('const', i, e instanceof TypeError)
  }
}
console.log(seen.join(','))

for (let i = 0; i < 3; i = i + 1) {
  try {
    let q = recsB[i]
    seen.push(q.n)
    q = recsB[(i + 1) % 4]
    seen.push(q.n)
  } catch (e) {
    console.log('let', i, e instanceof TypeError)
  }
}
console.log(seen.join(','))

function drop(): void {
  v = recsD[1]
}
for (let i = 0; i < 2; i = i + 1) {
  try {
    var v = recsD[2]
    seen.push(v.n)
    drop()
    seen.push(v.n)
  } catch (e) {
    console.log('var', i, e instanceof TypeError)
  }
}
console.log(seen.join(','))

{
  const b = recsE[3]
  try {
    console.log(b.n)
  } catch (e) {
    console.log('block', e instanceof TypeError)
  }
  try {
    console.log(b.key)
  } catch (e) {
    console.log('block again', e instanceof TypeError)
  }
}
