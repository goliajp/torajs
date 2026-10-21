// A function called with an `any` argument for a `string` parameter
// runs as a copy whose parameter is `any`. In that copy a binding
// assigned a value computed from the parameter (`i = i + t.length`)
// holds whatever that value is, so its reads are `any` there too —
// including the index a string method takes.

function inside(s: string, t: string): number {
  const out: number[] = []
  try {
    let i = 0
    while (i < s.length) {
      out.push(s.charCodeAt(i))
      if (i === 3) {
        const n = t.length
        i = i + n
      }
      i = i + 1
    }
  } catch (e) {
    console.log('inside', out.length, out[3], e instanceof TypeError)
  }
  return out.length
}

function at(s: string, t: string): number {
  let i = 0
  i = i + t.length
  return s.charCodeAt(i)
}

function label(s: string, t: string): string {
  let k = 0
  k = k + t.length
  return s + ':' + k + ':' + typeof k
}

const none: any = undefined
const two: any = 'zz'
const arr: any = [1, 2, 3]
console.log(inside('abcdefgh', none))
console.log(inside('abcdefgh', two))
console.log(inside('abcdefgh', 'typed'))
console.log(at('abcdefgh', two), at('abcdefgh', 'q'))
console.log(label('x', two), label('y', arr), label('z', 'abcd'))
