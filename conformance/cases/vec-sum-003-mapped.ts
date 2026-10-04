// A sum of `x + offset` through a closure the loop inlines: the
// element and the mapped value both have range guards, and the offset
// may be negative while every mapped value stays non-negative. Each
// sum has its own function, called once, as in the closure-counter
// bench case.
function loopSum(xs: number[], offset: number): number {
  const add = (x: number): number => x + offset
  let sum = 0
  for (let i = 0; i < xs.length; i = i + 1) {
    sum = sum + add(xs[i])
  }
  return sum
}
console.log(loopSum([9007199254740991, 6, 0], -2))

function loopSum0(xs: number[], offset: number): number {
  const add = (x: number): number => x + offset
  let sum = 0
  for (let i = 0; i < xs.length; i = i + 1) {
    sum = sum + add(xs[i])
  }
  return sum
}
const n0 = parseInt(process.argv[2] ?? '300', 10)
const off0 = parseInt(process.argv[3] ?? '-2', 10)
const at0 = parseInt(process.argv[4] ?? '0', 10)
const xs0: number[] = []
for (let i = 0; i < n0; i = i + 1) {
  if (i === at0) {
    xs0.push(2)
  } else {
    xs0.push(i)
  }
}
console.log(n0, off0, loopSum0(xs0, off0))

function loopSum1(xs: number[], offset: number): number {
  const add = (x: number): number => x + offset
  let sum = 0
  for (let i = 0; i < xs.length; i = i + 1) {
    sum = sum + add(xs[i])
  }
  return sum
}
const n1 = parseInt(process.argv[2] ?? '1000', 10)
const off1 = parseInt(process.argv[3] ?? '7', 10)
const at1 = parseInt(process.argv[4] ?? '-1', 10)
const xs1: number[] = []
for (let i = 0; i < n1; i = i + 1) {
  if (i === at1) {
    xs1.push(0)
  } else {
    xs1.push(i)
  }
}
console.log(n1, off1, loopSum1(xs1, off1))

function loopSum2(xs: number[], offset: number): number {
  const add = (x: number): number => x + offset
  let sum = 0
  for (let i = 0; i < xs.length; i = i + 1) {
    sum = sum + add(xs[i])
  }
  return sum
}
const n2 = parseInt(process.argv[2] ?? '300', 10)
const off2 = parseInt(process.argv[3] ?? '-1', 10)
const at2 = parseInt(process.argv[4] ?? '-1', 10)
const xs2: number[] = []
for (let i = 0; i < n2; i = i + 1) {
  if (i === at2) {
    xs2.push(0)
  } else {
    xs2.push(i)
  }
}
console.log(n2, off2, loopSum2(xs2, off2))

function loopSum3(xs: number[], offset: number): number {
  const add = (x: number): number => x + offset
  let sum = 0
  for (let i = 0; i < xs.length; i = i + 1) {
    sum = sum + add(xs[i])
  }
  return sum
}
const n3 = parseInt(process.argv[2] ?? '300', 10)
const off3 = parseInt(process.argv[3] ?? '-5', 10)
const at3 = parseInt(process.argv[4] ?? '3', 10)
const xs3: number[] = []
for (let i = 0; i < n3; i = i + 1) {
  if (i === at3) {
    xs3.push(5)
  } else {
    xs3.push(i)
  }
}
console.log(n3, off3, loopSum3(xs3, off3))

function loopSum4(xs: number[], offset: number): number {
  const add = (x: number): number => x + offset
  let sum = 0
  for (let i = 0; i < xs.length; i = i + 1) {
    sum = sum + add(xs[i])
  }
  return sum
}
const n4 = parseInt(process.argv[2] ?? '300', 10)
const off4 = parseInt(process.argv[3] ?? '3', 10)
const at4 = parseInt(process.argv[4] ?? '200', 10)
const xs4: number[] = []
for (let i = 0; i < n4; i = i + 1) {
  if (i === at4) {
    xs4.push(9007199254700992)
  } else {
    xs4.push(i)
  }
}
console.log(n4, off4, loopSum4(xs4, off4))

function loopSum5(xs: number[], offset: number): number {
  const add = (x: number): number => x + offset
  let sum = 0
  for (let i = 0; i < xs.length; i = i + 1) {
    sum = sum + add(xs[i])
  }
  return sum
}
const n5 = parseInt(process.argv[2] ?? '300', 10)
const off5 = parseInt(process.argv[3] ?? '0', 10)
const at5 = parseInt(process.argv[4] ?? '200', 10)
const xs5: number[] = []
for (let i = 0; i < n5; i = i + 1) {
  if (i === at5) {
    xs5.push(9007199254740990)
  } else {
    xs5.push(i)
  }
}
console.log(n5, off5, loopSum5(xs5, off5))

function loopSum6(xs: number[], offset: number): number {
  const add = (x: number): number => x + offset
  let sum = 0
  for (let i = 0; i < xs.length; i = i + 1) {
    sum = sum + add(xs[i])
  }
  return sum
}
const n6 = parseInt(process.argv[2] ?? '300', 10)
const off6 = parseInt(process.argv[3] ?? '-1', 10)
const at6 = parseInt(process.argv[4] ?? '8', 10)
const xs6: number[] = []
for (let i = 0; i < n6; i = i + 1) {
  if (i === at6) {
    xs6.push(9007199254740992)
  } else {
    xs6.push(i)
  }
}
console.log(n6, off6, loopSum6(xs6, off6))

function loopSum7(xs: number[], offset: number): number {
  const add = (x: number): number => x + offset
  let sum = 0
  for (let i = 0; i < xs.length; i = i + 1) {
    sum = sum + add(xs[i])
  }
  return sum
}
const n7 = parseInt(process.argv[2] ?? '13', 10)
const off7 = parseInt(process.argv[3] ?? '2', 10)
const at7 = parseInt(process.argv[4] ?? '-1', 10)
const xs7: number[] = []
for (let i = 0; i < n7; i = i + 1) {
  if (i === at7) {
    xs7.push(0)
  } else {
    xs7.push(i)
  }
}
console.log(n7, off7, loopSum7(xs7, off7))
