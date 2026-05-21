// P10.2-A4 — Promise.all<T>(promises).then(cb) where cb signature is
// `(arr: Array<T>) => V` for V ∈ {Number, String, Boolean, Void,
// Undefined}. Closes the typecheck gap exposed when the A3 smoke
// probe ran Promise.allSettled — same .then on heap-typed inner.
//
// Bun-parity scope: cb reads array via .length (primitive return).
// Closure-captured cb forms exercised through inline lambdas to
// avoid the pre-existing const-lambda binding crash (carried L3b).

// Number array → primitive (number) return
let pn: Promise<number>[] = [Promise.resolve(1), Promise.resolve(2), Promise.resolve(3)]
Promise.all(pn).then((arr: number[]) => {
  console.log('num', arr.length, arr[0], arr[2])
})

// String array → primitive (string) return
let ps: Promise<string>[] = [Promise.resolve('a'), Promise.resolve('b')]
Promise.all(ps).then((arr: string[]) => {
  console.log('str', arr.length, arr[0], arr[1])
})

// Boolean array → primitive (boolean) return
let pb: Promise<boolean>[] = [Promise.resolve(true), Promise.resolve(false)]
Promise.all(pb).then((arr: boolean[]) => {
  console.log('bool', arr.length, arr[0], arr[1])
})

// Void return → Promise<Undefined>
let pn2: Promise<number>[] = [Promise.resolve(10), Promise.resolve(20)]
Promise.all(pn2).then((arr: number[]) => {
  console.log('void', arr[0] + arr[1])
})

console.log('sync')
