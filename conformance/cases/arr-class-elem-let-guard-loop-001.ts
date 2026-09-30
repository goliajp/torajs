// The `let t = xs[i]; t.kind` shape inside loops: a `break` after a
// guard, a guard before the loop with a write inside it, a for-of
// binding, a switch clause, and a labeled break — every read that a
// loop-carried or branch-only value can reach still tests.

class Tok {
  kind: number
  value: number
  constructor(kind: number, value: number) {
    this.kind = kind
    this.value = value
  }
}

const xs: Tok[] = [new Tok(1, 10), new Tok(2, 20), new Tok(0, 30), new Tok(4, 40)]

function firstZero(): number {
  let i = 0
  while (true) {
    const t = xs[i]
    if (t === undefined) return -1
    if (t.kind === 0) break
    i = i + 1
  }
  return xs[i].value
}
console.log(firstZero())

function walkPast(limit: number): string {
  try {
    let n = 0
    for (let i = 0; i < limit; i = i + 1) {
      const t = xs[i]
      if (t.kind === 4) break
      n = n + t.value
    }
    return "n=" + n
  } catch (e: any) {
    return e instanceof TypeError ? "TypeError" : "other"
  }
}
console.log(walkPast(2), walkPast(4), walkPast(9))

function guardedBeforeLoop(): string {
  try {
    let t = xs[0]
    let s = t.kind
    for (let i = 1; i < 6; i = i + 1) {
      s = s + t.value
      t = xs[i]
    }
    return "s=" + s
  } catch (e: any) {
    return e instanceof TypeError ? "TypeError" : "other"
  }
}
console.log(guardedBeforeLoop())

function forOfKinds(): string {
  let out = ""
  for (const t of xs) {
    out = out + t.kind + ","
    out = out + t.value + ";"
  }
  return out
}
console.log(forOfKinds())

function viaSwitch(i: number): string {
  const t = xs[i]
  try {
    switch (i) {
      case 0:
        return "zero:" + t.kind
      case 9:
        return "nine:" + t.value
      default:
        return "other:" + t.kind
    }
  } catch (e: any) {
    return e instanceof TypeError ? "TypeError" : "other"
  }
}
console.log(viaSwitch(0), viaSwitch(1), viaSwitch(9))

function labeled(): string {
  let seen = ""
  outer: for (let i = 0; i < 4; i = i + 1) {
    const t = xs[i]
    for (let j = 0; j < 3; j = j + 1) {
      if (t.kind === 0) break outer
      seen = seen + t.value
    }
  }
  return seen
}
console.log(labeled())

function doWhileRead(): string {
  try {
    let i = 0
    let s = 0
    do {
      const t = xs[i]
      s = s + t.value
      i = i + 1
    } while (i < 5)
    return "s=" + s
  } catch (e: any) {
    return e instanceof TypeError ? "TypeError" : "other"
  }
}
console.log(doWhileRead())
