// A tail call may leave through a callee that hands a throw back in a
// register only when the caller does too; the throw must still reach
// the catch whichever side differs.
function g(n: number): number {
  if (n > 3) throw new Error('g' + n)
  return n * 2
}

function k(n: number): number {
  if (n > 5) throw new Error('k' + n)
  return n + 1
}
const kref = k

function viaG(n: number): number {
  if (n === -1) throw new Error('viaG')
  return g(n)
}

function viaK(n: number): number {
  if (n === -1) throw new Error('viaK')
  return k(n)
}

function tailToG(n: number): number {
  return g(n + 1)
}
const tref = tailToG

function plainTail(n: number): number {
  return g(n)
}

function run(label: string, f: (n: number) => number, n: number): void {
  try {
    console.log(label, f(n))
  } catch (e) {
    console.log(label, 'caught', (e as Error).message)
  }
}

function loop(rounds: number): string {
  let a = 3
  let b = 5
  let c = 7
  let d = 11
  let total = 0
  for (let i = 0; i < rounds; i++) {
    try {
      total += viaG(i)
    } catch (e) {
      total += 1000 + a
    }
    try {
      total += viaK(i)
    } catch (e) {
      total += 2000 + b
    }
    try {
      total += tailToG(i)
    } catch (e) {
      total += 3000 + c
    }
    try {
      total += plainTail(i)
    } catch (e) {
      total += 4000 + d
    }
    a = (a * 3 + i) % 10007
    b = (b * 5 + i) % 10009
    c = c + a - b
    d = d ^ c
  }
  return total + ' ' + a + ' ' + b + ' ' + c + ' ' + d
}

console.log(loop(8))
run('kref', kref, 9)
run('tref', tref, 9)
run('viaK', viaK, 2)
run('viaK', viaK, 6)
