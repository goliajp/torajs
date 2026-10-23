// A local written between a read past the end of an array of in-place
// instances and the field read of the view: the store of the local
// happens before the TypeError, and with the TypeError caught inside
// the loop the next pass reads the updated local.

class Tok {
  kind: number
  value: number
  constructor(kind: number, value: number) {
    this.kind = kind
    this.value = value
  }
}

class Box {
  last: number
  constructor() {
    this.last = -1
  }
}

function run(toks: Tok[], n: number, box: Box): number {
  let total = 0
  let k = 0
  for (let i = 0; i < n; i = i + 1) {
    const t = toks[i]
    k = k + 1
    box.last = k
    total = total + t.value
  }
  return total
}

function runCaught(toks: Tok[], n: number, box: Box): number {
  let total = 0
  let k = 0
  for (let i = 0; i < n; i = i + 1) {
    try {
      const t = toks[i]
      k = k + 1
      box.last = k
      total = total + t.value
    } catch (e) {
      total = total + 1000
    }
  }
  return total + k * 100000
}

const toks: Tok[] = []
for (let i = 0; i < 4; i = i + 1) toks.push(new Tok(i % 3, i + 1))
const box = new Box()
console.log('ok ' + run(toks, 4, box) + ' ' + box.last)
try {
  console.log('ran ' + run(toks, 6, box))
} catch (e) {
  console.log('caught ' + (e instanceof TypeError) + ' ' + box.last)
}
const box2 = new Box()
console.log('inner ' + runCaught(toks, 7, box2) + ' ' + box2.last)
