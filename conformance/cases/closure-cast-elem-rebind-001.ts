// A block-scoped binding cast from an element and rebound to
// `undefined` by a closure: reading through it throws TypeError on
// every pass. The binding released the element at each iteration's
// end, so the second pass read a freed record (exit 139).
type Rec = { key: string; n: number }
const recsC: (Rec | undefined)[] = [{ key: 'a', n: 1 }, undefined]
const seen: number[] = []
for (let i = 0; i < 3; i = i + 1) {
  try {
    let w = recsC[0] as Rec
    const clear = () => {
      w = recsC[1] as Rec
    }
    seen.push(w.n)
    clear()
    seen.push(w.n)
  } catch (e) {
    console.log('closure', i, e instanceof TypeError)
  }
}
console.log(seen.join(','))
