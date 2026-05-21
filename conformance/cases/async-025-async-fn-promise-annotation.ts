// P10.3-A2 — async fn with idiomatic `Promise<T>` return annotation.
// Pre-fix tora double-wrapped the user-declared type to
// `Promise<Promise<T>>` and rejected every `return e;` with
// "return type mismatch: function expects Promise(Promise(Number)),
// got Promise(Number)". The fix strips a leading `Promise<...>`
// wrapper in desugar_async so both annotation styles work:
//   async function f(): T            (inner-T MVP form)
//   async function f(): Promise<T>   (idiomatic TS form)
// Body return rewrite + tail-fallback still produce a single-wrap
// Promise<T>, matching the resolved fn signature in both cases.

// Form 1 — `Promise<T>` annotation, return literal
async function getOne(): Promise<number> {
  return 1
}
let v1 = await getOne()
console.log('one', v1)

// Form 2 — `Promise<T>` annotation, await inside body
async function double(p: Promise<number>): Promise<number> {
  const v = await p
  return v * 2
}
let v2 = await double(Promise.resolve(5))
console.log('double', v2)

// Form 3 — `Promise<string>` for heap inner T
async function greet(name: string): Promise<string> {
  return 'hi ' + name
}
let v3 = await greet('takagi')
console.log('greet', v3)

// Form 4 — inner-T (`: T`) annotation regression guard (pre-existing MVP form)
async function inner(): number {
  return 42
}
let v4 = await inner()
console.log('inner-T', v4)
