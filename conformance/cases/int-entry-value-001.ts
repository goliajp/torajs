// a loop reading a number it knows nothing about — the modulus of a
// `%`, a bound it also adds — runs on the integer image of that
// number when it is one, and exactly as before when it is not

function gcd(a: number, b: number): number {
  while (b !== 0) {
    const t = b
    b = a % b
    a = t
  }
  return a
}
function gcdSum(n: number, target: number): number {
  let total = 0
  for (let i = 1; i <= n; i = i + 1) {
    total = total + gcd(i, target)
  }
  return total
}
console.log(gcdSum(50, Number('1234567')), gcdSum(50, Number('0')), gcdSum(50, Number('-0')), gcdSum(50, Number('-6')), gcdSum(50, Number('2.5')))

// the value added each trip: whatever it is, the sum is the float one
function addEach(n: number, k: number): number {
  let s = 0
  for (let i = 0; i < n; i = i + 1) {
    s += i + k
  }
  return s
}
console.log(addEach(10, Number('x')), addEach(10, Number('Infinity')), addEach(10, Number('-0')), addEach(10, Number('2.5')))
console.log(addEach(10, Number('9007199254740992')), addEach(10, Number('9223372036854775808')))

// the sign of a zero survives the loop
function keepZero(n: number, k: number): number {
  let z = k
  for (let i = 0; i < n; i = i + 1) {
    z = z * 1
  }
  return 1 / z
}
console.log(keepZero(3, Number('-0')), keepZero(3, Number('0')))

// how many multiples: the modulus 0, negative, positive
function multiples(n: number, m: number): number {
  let count = 0
  for (let i = 0; i < n; i = i + 1) {
    if (i % m === 0) count = count + 1
  }
  return count
}
console.log(multiples(100, Number('0')), multiples(100, Number('-3')), multiples(100, Number('7')), multiples(100, Number('2.5')))

// an index taken modulo a pool size
function pooled(n: number, pool: number): number {
  const xs: number[] = []
  for (let i = 0; i < 100; i = i + 1) xs.push(i * 3)
  let s = 0
  for (let i = 0; i < n; i = i + 1) {
    const x = xs[i % pool]
    s = s + (x === undefined ? -1 : x)
  }
  return s
}
console.log(pooled(250, Number('100')), pooled(250, Number('0')), pooled(250, Number('7')))

// a bound the loop also reads
function boundAndValue(n: number): number {
  let s = 0
  for (let i = 0; i < n; i = i + 1) {
    s = s + n - i
  }
  return s
}
console.log(boundAndValue(Number('10')), boundAndValue(Number('10.5')), boundAndValue(Number('-2')))
