// a counting loop whose bound is a number only known at run time:
// the trip count is what comparing the counter with that number
// gives, whatever the number turns out to be

function upTo(n: number): number {
  let trips = 0
  for (let i = 0; i < n; i = i + 1) {
    trips = trips + 1
    if (i === 5) break
  }
  return trips
}

function through(n: number): number {
  let trips = 0
  for (let i = 0; i <= n; i = i + 1) {
    trips = trips + 1
    if (i === 5) break
  }
  return trips
}

function fromBelow(n: number): number {
  let sum = 0
  for (let i = -5; i < n; i = i + 1) {
    sum = sum + i
  }
  return sum
}

function downTo(n: number): number {
  let trips = 0
  for (let i = 5; i > n; i = i - 1) {
    trips = trips + 1
  }
  return trips
}

function downThrough(n: number): number {
  let trips = 0
  for (let i = 5; i >= n; i = i - 1) {
    trips = trips + 1
  }
  return trips
}

function mirrored(n: number): number {
  let trips = 0
  for (let i = 0; n > i; i = i + 1) {
    trips = trips + 1
    if (i === 5) break
  }
  return trips
}

const inputs = [
  '2.5', '-2.5', '0.5', '3', '0', '-0', 'x', '1e-7',
  '9007199254740991', '9007199254740992', '4503599627370496', '4503599627370497',
  '9223372036854775808', 'Infinity', '-Infinity', '2147483648', '2147483647',
]
for (let k = 0; k < inputs.length; k = k + 1) {
  const n = Number(inputs[k])
  console.log(
    inputs[k], upTo(n), through(n), fromBelow(n < 100 ? n : 3), downTo(n > -100 ? n : 3),
    downThrough(n > -100 ? n : 3), mirrored(n),
  )
}

// bitwise work in the body: the counter must stay a 32-bit value
function bits(n: number): number {
  let total = 0
  for (let i = 0; i < n; i = i + 1) {
    let x = i
    let count = 0
    while (x !== 0) {
      x = x & (x - 1)
      count = count + 1
    }
    total = total + count
  }
  return total
}
console.log(bits(parseInt('100000', 10)), bits(Number('10.5')), bits(Number('x')))

// the value that ended the loop is read after it
function last(n: number): number {
  let i = 0
  let sum = 0
  while (i < n) {
    sum = sum + i * 2
    i = i + 3
  }
  return i * 1000 + sum
}
console.log(last(Number('10')), last(Number('10.5')), last(Number('-1')), last(Number('x')))

// two loops on one bound, the second reading what the first left
function twoLoops(n: number): number {
  let acc = 1
  for (let i = 0; i < n; i = i + 1) {
    acc = acc + i
  }
  for (let j = 0; j <= n; j = j + 1) {
    acc = acc + j * acc % 7
  }
  return acc
}
console.log(twoLoops(Number('6')), twoLoops(Number('6.5')), twoLoops(Number('x')))

// a loop under a branch: the code after it also runs without it
function maybe(n: number, run: boolean): number {
  let sum = 7
  if (run) {
    for (let i = 0; i < n; i = i + 1) {
      sum = sum + i
    }
  }
  return sum
}
console.log(maybe(Number('4'), true), maybe(Number('4'), false), maybe(Number('4.5'), true))

// nested: the inner bound is the outer counter, the outer bound opaque
function nested(n: number): number {
  let sum = 0
  for (let i = 0; i < n; i = i + 1) {
    for (let j = 0; j < i; j = j + 1) {
      sum = sum + j
    }
  }
  return sum
}
console.log(nested(Number('12')), nested(Number('12.2')), nested(Number('-3')))

// counter starting just below the window's top
function nearTop(n: number): number {
  let trips = 0
  for (let i = 4503599627370494; i < n; i = i + 1) {
    trips = trips + 1
    if (trips === 4) break
  }
  return trips
}
console.log(nearTop(Number('4503599627370496')), nearTop(Number('4503599627370497')), nearTop(Number('9007199254740991')))
