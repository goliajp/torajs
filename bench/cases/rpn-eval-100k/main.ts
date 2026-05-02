function evalRpn(expr: string): number {
  const stack: number[] = [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]
  let sp = 0
  const parts = expr.split(' ')
  for (let i = 0; i < parts.length; i = i + 1) {
    const tok = parts[i]
    const c0 = tok.charCodeAt(0)
    if (c0 >= 48 && c0 <= 57) {
      stack[sp] = c0 - 48
      sp = sp + 1
    } else {
      const b = stack[sp - 1]
      const a = stack[sp - 2]
      sp = sp - 2
      let r = 0
      if (c0 === 43) r = a + b
      else if (c0 === 45) r = a - b
      else r = a * b
      stack[sp] = r
      sp = sp + 1
    }
  }
  return stack[0]
}

let total = 0
const n = 100000
for (let i = 0; i < n; i = i + 1) {
  total = total + evalRpn('3 4 + 2 * 5 +')
}
console.log(total)
