class Fail {
  code: number
  constructor(code: number) {
    this.code = code
  }
}

let finals = 0

function descend(d: number, v: number, every: number): number {
  if (d === 0) {
    if (v % every === 0) throw new Fail(v)
    return v
  }
  try {
    return descend(d - 1, v + 1, every) + 1
  } finally {
    finals = finals + 1
  }
}

const n = parseInt(process.argv[2], 10)
const depth = parseInt(process.argv[3], 10)
const every = parseInt(process.argv[4], 10)

let total = 0
for (let i = 0; i < n; i = i + 1) {
  try {
    total = total + descend(depth, i, every)
  } catch (e) {
    total = total + (e as Fail).code
  }
}
console.log(total, finals)
