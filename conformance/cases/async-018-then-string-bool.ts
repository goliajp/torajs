// T-19.g (v0.5.0) — Promise<T>.then(cb) for T ∈ String / Boolean
// (the Number variant covered by async-002). cb signature is
// `(v: T) => T` — same T in/out — no generic U yet (T-15.g.4
// substrate). Runtime helper packs i64 through Str* / i1 cleanly
// since both round-trip through 64-bit values without aliasing.
//
// Also covers the Bun.file().text().then(cb) source — i.e. the
// `.then` source-detection knows fs/promises + Bun.file().text/.exists
// produce built-in Promises (the async-002 chain only exercised
// Promise.resolve / Promise.then static-source paths).

import { writeFile } from 'fs/promises'
await writeFile('/tmp/torajs-then-fixture.txt', 'hello world')

function shout(s: string): string { return s + '!' }
function flip(b: boolean): boolean { return !b }

let str_p = Promise.resolve('hi').then(shout)
console.log(await str_p)                   // hi!

let bool_p = Promise.resolve(true).then(flip)
console.log(await bool_p)                  // false

let chain_p = Bun.file('/tmp/torajs-then-fixture.txt').text().then(shout)
console.log(await chain_p)                 // hello world!
