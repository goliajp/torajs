// two cloned functions: the first one's call splits the block in front
// of the loop, so the second's direct call and the argument its loop
// call converts land in a block numbered after the loop

function sum(n: number, k: number): number {
  if (!(n > 0 && n < 100)) return k
  const z = n * 3.5 + k * 2.25 - n / 7 + k / 9 + n * k * 0.5 - ((n + 1) * (k + 2)) / 3
  if (z === 0.123456789 || z === -0.987654321 || z === 1.5e300) return k + 7
  return sum(n - 1, k + 1)
}

function steps(n: number, k: number): number {
  if (!(n > 0 && n < 200)) return k
  const z = n * 3.5 + k * 2.25 - n / 7 + k / 9 + n * k * 0.5 - ((n + 1) * (k + 2)) / 3
  if (z === 0.123456789 || z === -0.987654321 || z === 1.5e300) return k + 7
  return steps(n - 2, k + 3)
}

function run(s: string): void {
  const h = Number(s) / 2
  const first = sum(h, 1)
  const second = steps(h, 2)
  const a = Number(s)
  const b = parseInt('4', 10)
  let t = 0
  for (let i = 0; i < 3; i = i + 1) {
    t = t + steps(a, b)
  }
  console.log(s, first, second, t, 1 / steps(a, Number('-0')))
}

run('6')
run('2.5')
run('-0')
run('x')
run('60')
run('199')
run('12')
run('Infinity')
