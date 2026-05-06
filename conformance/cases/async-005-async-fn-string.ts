// T-15.h + T-15.g.5 — async fn returning Promise<string>. The
// desugar rewrites `return e` to `return Promise.resolve(e)`; the
// Call-arm dispatch routes Promise.resolve(<string>) to the heap
// variant.

async function greet(): string {
  return 'hello'
}

async function namePart(): string {
  return 'torajs'
}

async function full(): string {
  let g: string = await greet()
  let n: string = await namePart()
  return g + ', ' + n + '!'
}

let r: string = await full()
console.log(r)
