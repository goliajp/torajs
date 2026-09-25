// non-escaping header cells promoted to the stack after inlining:
// captured fields must still read the stored values, per-iteration
// cells must not bleed into each other, pointer identity must hold

function sumWith(xs: number[], offset: number): number {
  const add = (x: number): number => x + offset
  let sum = 0
  for (let i = 0; i < xs.length; i = i + 1) {
    sum = sum + add(xs[i])
  }
  return sum
}

function perIteration(n: number): number {
  let total = 0
  for (let i = 0; i < n; i = i + 1) {
    const k = i * 3
    const f = (x: number): number => x * k
    total = total + f(2)
  }
  return total
}

function point(n: number): number {
  let acc = 0
  for (let i = 0; i < n; i = i + 1) {
    const p = { x: i, y: i + 1 }
    acc = acc + p.x * p.y
  }
  return acc
}

function identity(): string {
  const f = (): number => 1
  const g = (): number => 1
  const h = f
  return `${f === g} ${f !== g} ${f === h} ${h === f}`
}

function strCapture(n: number): number {
  const tag = "ab"
  const len = (x: number): number => x + tag.length
  let s = 0
  for (let i = 0; i < n; i = i + 1) {
    s = s + len(i)
  }
  return s
}

const xs: number[] = []
for (let i = 0; i < 1000; i = i + 1) {
  xs.push(i)
}
console.log(sumWith(xs, 2))
console.log(sumWith(xs, -7))
console.log(perIteration(100))
console.log(point(50))
console.log(identity())
console.log(strCapture(10))
