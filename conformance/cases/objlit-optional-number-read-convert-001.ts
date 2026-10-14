// A class field written through the any lane holds the written value in
// its sidecar; the typed read converts that value into the field's
// declared layout, and a `null` the value carries for an optional
// `number` member of it is that member's value, read through the field.
interface Inner {
  a?: number
  b: number
}
class W {
  inner: Inner | null = null
  list: Inner[] | null = null
}

const w = new W()
const text = process.argv.length > 99 ? "{}" : '{"a":null,"b":2}'
;(w as any).inner = JSON.parse(text)
const i = w.inner
if (i !== null) {
  console.log(i.a, i.b, i.a === null, typeof i.a, "a" in i, i.a ?? "dflt")
  console.log(JSON.stringify(i), Object.keys(i).join(","))
}
console.log(w.inner?.a, w.inner?.b)

const many = process.argv.length > 99 ? "[]" : '[{"a":null,"b":1},{"b":2},{"a":3,"b":4}]'
;(w as any).list = JSON.parse(many)
const xs = w.list
if (xs !== null) {
  for (const x of xs) console.log(x.a, typeof x.a, "a" in x, x.a ?? -1)
}

