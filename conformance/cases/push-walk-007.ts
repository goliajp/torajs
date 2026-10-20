// A receiver that the same body locks right before the walk: the
// append that finds no room raises the push's TypeError, after the
// effects of the trips before it, and a walk that appends nothing
// raises nothing.
let spaces = 0
function frozen(s: string, out: number[]): number {
  Object.freeze(out)
  let i = 0
  while (i < s.length) {
    const c = s.charCodeAt(i)
    if (c === 32) {
      spaces = spaces + 1
      i = i + 1
    } else {
      out.push(c)
      i = i + 1
    }
  }
  return out.length
}
function sealed(s: string, out: number[]): number {
  Object.seal(out)
  let i = 0
  while (i < s.length) {
    const c = s.charCodeAt(i)
    if (c === 32) {
      spaces = spaces + 1
      i = i + 1
    } else {
      out.push(c)
      out.push(i)
      i = i + 1
    }
  }
  return out.length
}
function closed(s: string, out: number[]): number {
  Object.preventExtensions(out)
  let i = 0
  while (i < s.length) {
    const c = s.charCodeAt(i)
    if (c === 32) {
      spaces = spaces + 1
      i = i + 1
    } else {
      out.push(c)
      i = i + 1
    }
  }
  return out.length
}
function attempt(which: number, s: string): string {
  const out: number[] = [1, 2]
  spaces = 0
  try {
    const n = which === 0 ? frozen(s, out) : which === 1 ? sealed(s, out) : closed(s, out)
    return 'done ' + spaces + ' ' + n
  } catch (e) {
    return (e as Error).constructor.name + ' ' + spaces + ' ' + out.length
  }
}
for (const s of ['  x y', '   ', '', 'ab']) {
  console.log(attempt(0, s), '|', attempt(1, s), '|', attempt(2, s))
}
