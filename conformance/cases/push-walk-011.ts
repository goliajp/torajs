// A string walk whose body touches a binding before its declaration
// in the same body: the ReferenceError leaves the loop after some
// appends, and the array the catch sees holds every one of them.

function reads(s: string, out: number[]): void {
  let i = 0
  while (i < s.length) {
    out.push(i)
    if (i === 2) {
      const c = d + 1
      i = i + c
    }
    const d = 5
    i = i + 1
  }
}

function writes(s: string, out: number[]): void {
  let i = 0
  while (i < s.length) {
    out.push(s.charCodeAt(i))
    if (i === 1) {
      z = 3
    }
    let z = 0
    i = i + 1
  }
}

const a: number[] = []
try {
  reads('abcdef', a)
} catch (e) {
  console.log(a.length, a.join(','), e instanceof ReferenceError)
}
a.push(42)
console.log(a.length, a[a.length - 1])
const b: number[] = [9]
try {
  writes('abcdef', b)
} catch (e) {
  console.log(b.length, b.join(','), e instanceof ReferenceError)
}
