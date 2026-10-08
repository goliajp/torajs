// a call inside a loop converts the arguments the loop does not
// change once, in front of the loop; an argument that is not an
// integer inside the window keeps every trip on the original

function sum(n: number, k: number): number {
  if (!(n > 0 && n < 100)) return k
  return sum(n - 1, k + 1)
}

const inputs = ['6', '2.5', '-0', 'x', 'Infinity', '-3', '9007199254740992', '0', '99']
for (const s of inputs) {
  const a = Number(s)
  const b = parseInt('4', 10)
  let t = 0
  for (let i = 0; i < 3; i = i + 1) {
    t = t + sum(a, b)
  }
  console.log(s, t, 1 / sum(a, Number('-0')))
}

// the rest reads locals, which a loop leaves alone
function main(): void {
  // the same arguments twice in one loop, and a loop that never runs
  const c = parseInt('7', 10)
  const z = Number('-0')
  let u = 0
  for (let i = 0; i < 4; i = i + 1) {
    u = u + sum(c, z) + sum(c, z)
  }
  for (let i = 0; i < parseInt('0', 10); i = i + 1) {
    u = u + sum(c, 1)
  }
  console.log(u, 1 / sum(parseInt('0', 10), z))

  // an argument the loop changes keeps the original
  let v = 0
  for (let i = 0; i < 6; i = i + 1) {
    v = v + sum(i * 0.5, c)
  }
  console.log(v)

  // invariant in the inner loop only
  let w = 0
  for (let i = 0; i < 3; i = i + 1) {
    const d = Number(String(i * 1.5))
    for (let j = 0; j < 2; j = j + 1) {
      w = w + sum(d, c)
    }
  }
  console.log(w)

  // invariant in both loops: converted in front of the outer one
  const e = parseInt('12', 10)
  let x = 0
  for (let i = 0; i < 3; i = i + 1) {
    for (let j = 0; j < 2; j = j + 1) {
      x = x + sum(e, c) + j
    }
  }
  console.log(x)
}
main()
