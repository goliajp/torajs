// A proven-live `t` stops being proven wherever the name comes to
// mean another value: an inner block's `const t` shadows the outer
// one and its guard says nothing about the outer binding; a switch
// clause is entered without running the guard in the clause before
// it; a `for` step runs after the body rewrote the binding.

class Tok {
  kind: number
  value: number
  constructor(kind: number, value: number) {
    this.kind = kind
    this.value = value
  }
}

const xs: Tok[] = [new Tok(1, 10), new Tok(2, 20), new Tok(3, 30)]

function shadowBlock(i: number, j: number): string {
  try {
    let t = xs[j]
    let k = 0
    {
      const t = xs[i]
      k = t.kind
    }
    return k + "/" + t.value
  } catch (e: any) {
    return e instanceof TypeError ? "TypeError" : "other"
  }
}
console.log(shadowBlock(0, 1), shadowBlock(0, 5), shadowBlock(5, 0))

function clauseEntry(i: number, which: number): string {
  try {
    const t = xs[i]
    switch (which) {
      case 1:
        return "kind=" + t.kind
      case 2:
        return "value=" + t.value
      default:
        return "none"
    }
  } catch (e: any) {
    return e instanceof TypeError ? "TypeError" : "other"
  }
}
console.log(clauseEntry(0, 1), clauseEntry(0, 2), clauseEntry(4, 2), clauseEntry(4, 3))

function stepReads(i: number): string {
  try {
    let t = xs[0]
    let n = 0
    for (; t.kind > 0 && n < 3; n += t.value) {
      t = xs[i]
    }
    return "n=" + n
  } catch (e: any) {
    return e instanceof TypeError ? "TypeError" : "other"
  }
}
console.log(stepReads(1), stepReads(5))
