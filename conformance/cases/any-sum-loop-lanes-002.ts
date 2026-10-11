// `any` accumulator loops whose lanes meet another loop, an outer
// loop, the edges of the number encoding, and every way out.

function mk(n: number): any[] {
  const a: any[] = []
  for (let i = 0; i < n; i++) a.push(i % 3 === 0 ? i + 0.5 : i)
  return a
}

// two loops side by side; the first has more blocks, the second
// starts from the first one's total and reads it in its body
function sideBySideBigFirst(xs: any[], ys: any[]): void {
  let s: any = 0
  for (let i = 0; i < xs.length; i++) {
    if (i % 7 === 0) {
      s = s + xs[i]
    } else if (i % 5 === 0) {
      s = s + 1
    } else {
      s = s + xs[i]
    }
  }
  let t: any = s
  for (let j = 0; j < ys.length; j++) t = t + ys[j]
  console.log(s, t)
  let u: any = 0
  for (let k = 0; k < ys.length; k++) u = u + ys[k] + s
  console.log(u)
}

// the same with the second loop the bigger one
function sideBySideSmallFirst(xs: any[], ys: any[]): void {
  let s: any = 0
  for (let i = 0; i < xs.length; i++) s = s + xs[i]
  let t: any = s
  for (let j = 0; j < ys.length; j++) {
    if (j % 7 === 0) {
      t = t + ys[j]
    } else if (j % 5 === 0) {
      t = t + s
    } else {
      t = t + ys[j]
    }
  }
  console.log(s, t)
  let u: any = 0
  for (let k = 0; k < ys.length; k++) u = u + s
  console.log(u)
}

// the accumulator lives across the outer loop
function outerKeeps(xs: any[]): void {
  let s: any = 0
  for (let r = 0; r < 3; r++) {
    for (let i = 0; i < xs.length; i++) s = s + xs[i]
    console.log('outer', r, s)
  }
  let d: any = 0.25
  let r = 0
  do {
    for (let i = 0; i < xs.length; i++) d = d + xs[i]
    console.log('do', r, d)
    r++
  } while (r < 3)
}

// the edges of the encoding: -0, NaN, Infinity, int32 overflow
function edges(): void {
  let t: any = -0
  const z: any[] = [-0, -0, -0]
  for (let i = 0; i < z.length; i++) t = t + z[i]
  console.log(Object.is(t, -0), Object.is(t, 0))
  let n: any = 1
  const ns: any[] = [2, NaN, 3]
  for (let i = 0; i < ns.length; i++) n = n + ns[i]
  console.log(n, Number.isNaN(n))
  let inf: any = 0
  const is: any[] = [1, Infinity, 2, -Infinity]
  for (let i = 0; i < is.length; i++) {
    inf = inf + is[i]
    console.log('inf', i, inf)
  }
  let big: any = 2147483647
  const bs: any[] = [1, -1, 1, -2]
  for (let i = 0; i < bs.length; i++) {
    big = big + bs[i]
    console.log('big', big, (big | 0) === big)
  }
  let neg: any = -2147483648
  const ms: any[] = [-1, 1]
  for (let i = 0; i < ms.length; i++) neg = neg + ms[i]
  console.log(neg, (neg | 0) === neg)
}

// several ways out, each reading the total
function exits(xs: any[]): void {
  let total: any = 0
  for (let i = 0; i < xs.length; i++) {
    if (i === 50) break
    total = total + xs[i]
  }
  console.log('break', total)
  total = 0
  outer: for (let r = 0; r < 4; r++) {
    for (let i = 0; i < xs.length; i++) {
      if (i === 10 + r) continue outer
      total = total + xs[i]
    }
  }
  console.log('labeled continue', total)
  const early = (ys: any[]): any => {
    let acc: any = 0
    for (let i = 0; i < ys.length; i++) {
      if (i === 33) return acc
      acc = acc + ys[i]
    }
    return -acc
  }
  console.log('return', early(xs), early(xs.slice(0, 20)))
}

// an array index getter throws in the middle of the fast part
function throwing(): void {
  const xs: any[] = mk(40)
  Object.defineProperty(xs, 25, {
    get() {
      throw new Error('boom at 25')
    },
  })
  let total: any = 0
  try {
    for (let i = 0; i < xs.length; i++) total = total + xs[i]
  } catch (e) {
    console.log('caught', (e as Error).message, total)
  }
  let pre: any = 0
  try {
    pre = pre + 100
    for (let i = 0; i < xs.length; i++) pre = pre + xs[i]
  } catch (e) {
    console.log('caught pre', (e as Error).message, pre)
  }
}

// a non-number addend halfway: the sum turns into a string
function turns(xs: any[]): void {
  let total: any = 0
  for (let i = 0; i < xs.length; i++) total = total + (i === 7 ? 'x' : xs[i])
  console.log(total)
}

sideBySideBigFirst(mk(1000), mk(100))
sideBySideSmallFirst(mk(1000), mk(100))
outerKeeps(mk(20))
edges()
exits(mk(100))
throwing()
turns(mk(12))
