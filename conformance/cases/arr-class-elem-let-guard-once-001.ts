// `let t = xs[i]` then member reads on `t`: the first read tests the
// binding, the ones after it on the same straight-line path do not —
// and every path that could change what `t` holds still tests: a
// reassignment, a read that runs only on one branch, a read in a
// catch clause, a read after an `if (t === undefined) return`.

class Tok {
  kind: number
  value: number
  constructor(kind: number, value: number) {
    this.kind = kind
    this.value = value
  }
}

const xs: Tok[] = [new Tok(1, 10), new Tok(2, 20), new Tok(3, 30)]

function describe(i: number): string {
  const t = xs[i]
  if (t === undefined) return "none"
  return t.kind + ":" + t.value + ":" + t.kind
}
console.log(describe(0), describe(2), describe(3), describe(-1))

function sumAfterReassign(i: number, j: number): string {
  try {
    let t = xs[i]
    let s = t.kind + t.value
    t = xs[j]
    s = s + t.kind
    return "sum=" + s
  } catch (e: any) {
    return e instanceof TypeError ? "TypeError" : "other"
  }
}
console.log(sumAfterReassign(0, 1), sumAfterReassign(0, 5), sumAfterReassign(5, 0))

function branchOnly(i: number, flag: boolean): string {
  try {
    const t = xs[i]
    const a = flag && t.kind === 1 ? "one" : "not-one"
    return a + "/" + t.value
  } catch (e: any) {
    return e instanceof TypeError ? "TypeError" : "other"
  }
}
console.log(branchOnly(0, true), branchOnly(0, false), branchOnly(9, false), branchOnly(9, true))

function inCatch(i: number): string {
  const t = xs[i]
  try {
    if (i === 1) throw new Error("boom")
    return "kind=" + t.kind
  } catch (e: any) {
    try {
      return "caught:" + t.value
    } catch (e2: any) {
      return e2 instanceof TypeError ? "TypeError-in-catch" : "other"
    }
  }
}
console.log(inCatch(0), inCatch(1), inCatch(7))

function ternaryThenRead(i: number, pick: boolean): string {
  try {
    const t = xs[i]
    const k = pick ? t.kind : -1
    return k + "/" + t.value
  } catch (e: any) {
    return e instanceof TypeError ? "TypeError" : "other"
  }
}
console.log(ternaryThenRead(1, true), ternaryThenRead(1, false), ternaryThenRead(4, false))
