// A generator local holding an index load off an `any`. The lift's
// initializer sniff reads `st[i]` through an `Index` arm that strips
// `[]` off the base's annotation, so an `any` base made it decline —
// and a decline pins the lifted field to `number`, which every later
// use of it then fails against. `__torajs_adstack_walk` (the injected
// `using` walker) is written exactly this way.
function* walk(st: any): any {
  let i = st.length - 1
  while (i >= 0) {
    const r = st[i]
    yield r.k
    i = i - 1
  }
}
const it = walk([{ k: 2 }, { k: 1 }, { k: 0 }])
console.log(it.next().value, it.next().value, it.next().value)

// The neighbours that must NOT move: a typed base keeps the precise
// element type the shared arm already answers, and a nested index
// composes through it.
function* nums(xs: number[]): any {
  const first = xs[0]
  yield first + 1
}
console.log(nums([41]).next().value)

function* rows(g: any): any {
  const row = g[0]
  const cell = row[1]
  yield cell.n
}
console.log(rows([[{ n: 0 }, { n: 7 }]]).next().value)
