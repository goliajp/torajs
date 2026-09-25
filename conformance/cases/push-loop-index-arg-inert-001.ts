// a push argument that reads an element of another array lets the
// fill loop keep its length word in a register; it must not when the
// read array can be the one being filled

function copy(src: number[]): number[] {
  const dst: number[] = []
  for (let i = 0; i < src.length; i++) {
    dst.push(src[i] * 2)
  }
  return dst
}

function shifted(src: number[]): number[] {
  const dst: number[] = []
  for (let i = 0; i < src.length; i++) {
    dst.push(src[i + 2])
  }
  return dst
}

function aliased(n: number): number[] {
  const a: number[] = [1, 2, 3]
  const b = a
  for (let i = 0; i < n; i++) {
    a.push(b[i + 1])
  }
  return a
}

function selfRead(n: number): number[] {
  const a: number[] = [5, 6]
  for (let i = 0; i < n; i++) {
    a.push(a[i] + a.length)
  }
  return a
}

function lockstep(n: number): string {
  const a: number[] = []
  const b: number[] = [7]
  for (let i = 0; i < n; i++) {
    a.push(b[i])
    b.push(i)
  }
  return `${a.join(",")} | ${b.join(",")}`
}

function flags(src: boolean[]): boolean[] {
  const out: boolean[] = []
  for (let i = 0; i < src.length; i++) {
    out.push(src[i])
  }
  return out
}

const base: number[] = []
for (let i = 0; i < 6; i++) {
  base.push(i * 10)
}
console.log(copy(base).join(","))
const sh = shifted(base)
console.log(sh.length, sh[0], sh[3], sh[4] === undefined, sh[5] === undefined)
console.log(aliased(4).join(","))
console.log(selfRead(4).join(","))
console.log(lockstep(4))
console.log(flags([true, false, true]).join(","))
