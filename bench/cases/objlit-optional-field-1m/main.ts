type Opt = { a?: number; b: number }

function make(i: number): Opt {
  if (i % 4 === 0) return { a: i, b: i }
  return { b: i }
}

const n = parseInt(process.argv[2], 10)
let total = 0
let created = 0
for (let i = 0; i < n; i = i + 1) {
  const o = make(i)
  if (!("a" in o)) {
    o.a = 2
    created = created + 1
  }
  o.b = o.b + 1
  total = total + (o.a ?? 0) + o.b
}
console.log(total, created)
