// Sum loops that must keep their scalar shape: a push or a call with a
// side effect in the body, a float array, a float starting total.
function sumAndLog(xs: number[], log: number[]): number {
  let s = 0
  for (let i = 0; i < xs.length; i = i + 1) {
    s = s + xs[i]
    log.push(s)
  }
  return s
}

let calls = 0
function counted(x: number): number {
  calls = calls + 1
  return x * 2
}

function sumCounted(xs: number[]): number {
  let s = 0
  for (let i = 0; i < xs.length; i = i + 1) {
    s = s + counted(xs[i])
  }
  return s
}

function sumFrom(xs: number[]): number {
  let sum = 0.5
  for (let i = 0; i < xs.length; i = i + 1) {
    sum = sum + xs[i]
  }
  return sum
}

function sum(xs: number[]): number {
  let s = 0
  for (let i = 0; i < xs.length; i = i + 1) {
    s = s + xs[i]
  }
  return s
}

const xs: number[] = []
for (let i = 0; i < 100; i = i + 1) {
  xs.push(i)
}
const log: number[] = []
console.log(sumAndLog(xs, log), log.length, log[99])
console.log(sumCounted(xs), calls)
console.log(sumFrom(xs))

const fs: number[] = []
for (let i = 0; i < 100; i = i + 1) {
  fs.push(i)
}
fs.push(0.5)
console.log(sum(fs))

const neg: number[] = []
for (let i = 0; i < 300; i = i + 1) {
  neg.push(i - 150)
}
console.log(sum(neg))

// negative elements in a top-level sum: with a bound known at compile
// time the sum is a plain i64 sum; with one read at run time every
// element has a range guard, and the first negative one sends the
// loop to its float copy
let t = 0
for (let i = 0; i < neg.length; i = i + 1) {
  t = t + neg[i]
}
const m = parseInt(process.argv[2] ?? '300', 10)
const neg2: number[] = []
for (let i = 0; i < m; i = i + 1) {
  neg2.push(i - 150)
}
let t2 = 0
for (let i = 0; i < neg2.length; i = i + 1) {
  t2 = t2 + neg2[i]
}
console.log(t, t2)
