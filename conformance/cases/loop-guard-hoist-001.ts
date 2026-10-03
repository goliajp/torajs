// `while (i < s.length)` guards `s` against the nullish spellings and
// reads its length and encoding on every iteration. For a receiver the
// loop does not replace, the guard and those reads move in front of
// the loop. What must not change: a nullish receiver still raises the
// TypeError at the loop, after everything before the loop has run and
// before anything inside it; an empty string runs the loop zero
// times; a wide string still reads code units; and a loop that
// replaces, grows or aliases its string keeps seeing the new one.
function scan(s: string): number {
  let n = 0
  let i = 0
  while (i < s.length) {
    const c = s.charCodeAt(i)
    if (c >= 48 && c <= 57) {
      let v = 0
      while (i < s.length) {
        const d = s.charCodeAt(i)
        if (d < 48 || d > 57) break
        v = v * 10 + (d - 48)
        i = i + 1
      }
      n = n + v
    } else {
      n = n + c
      i = i + 1
    }
  }
  return n
}

const log: string[] = []
function scanLogged(s: string, tag: string): number {
  log.push('enter ' + tag)
  let n = 0
  for (let i = 0; i < s.length; i = i + 1) {
    log.push('iter ' + tag)
    n = n + s.charCodeAt(i)
  }
  log.push('leave ' + tag)
  return n
}

console.log(scan('12 ab 345'), scan(''), scan('7'), scan('é12'), scan('日本12語3'))

const text = 'xyz'
const parts: string[] = ['ab', text[7], '', 'c']
for (let k = 0; k < parts.length; k = k + 1) {
  try {
    console.log(scanLogged(parts[k], 'p' + k))
  } catch (e) {
    console.log((e as Error).name, e instanceof TypeError)
  }
}
console.log(log.join('|'))

function shrink(s: string): number {
  let steps = 0
  while (s.length > 0) {
    s = s.slice(1)
    steps = steps + 1
  }
  return steps
}

function grow(s: string): number {
  let k = 0
  while (s.length < 20) {
    s = s + 'ab'
    k = k + s.charCodeAt(s.length - 1)
  }
  return k + s.length
}

function alias(s: string): number {
  let t = s
  let n = 0
  for (let i = 0; i < s.length; i = i + 1) {
    t = t + 'x'
    n = n + s.length * 100 + t.length
  }
  return n
}

console.log(shrink('hello'), grow('a'), grow(''), alias('abc'), alias(''))

function sum(xs: number[]): number {
  let t = 0
  for (let i = 0; i < xs.length; i = i + 1) t = t + xs[i]
  return t
}

function fill(xs: number[]): number {
  let i = 0
  while (xs.length < 10) {
    xs.push(i)
    i = i + 1
  }
  return xs.length * 100 + i
}

const none: any = undefined
console.log(sum([1, 2, 3]), sum([]), fill([5, 6]), fill([]))
try {
  console.log(sum(none))
} catch (e) {
  console.log((e as Error).name)
}
