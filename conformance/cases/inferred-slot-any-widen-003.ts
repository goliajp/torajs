// An outer binding reassigned inside a closure body: the assign the
// closure makes widens the outer declaration just like one made in
// place.

const vals: any[] = [1, 'b', 2]
let total = 0
vals.forEach((v) => {
  total = total + v
})
console.log(total, typeof total)

function build(items: any[]) {
  let acc = 0
  const add = (x: any) => {
    acc = acc + x
  }
  for (const it of items) add(it)
  return acc
}
console.log(build([1, 2, 3]), build(['a', 1]))

let depth = 0
const nested = () => {
  const inner = () => {
    depth = depth + vals[1]
  }
  inner()
}
nested()
console.log(depth, typeof depth)

// a closure that makes itself again inside its own body
let n = 0
const again = (k: number): any => {
  n = n + 1
  return k > 0 ? again(k - 1) : n
}
console.log(again(3), n)
