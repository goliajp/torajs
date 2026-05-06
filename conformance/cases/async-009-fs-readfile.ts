// T-18.a (v0.5.0) — `fs/promises` module. Each async method calls
// the matching sync helper from `fs.<X>Sync` then wraps in
// Promise.resolve(...). MVP "synchronous-then-resolve" — real I/O
// suspension needs T-16 state-machine async/await.
//
// Bun-parity scope: `writeFile` / `exists` / `unlink` / `mkdir` /
// `appendFile` all match bun byte-identically. `readFile` returns
// `string` in tr but `Buffer` in bun (no Buffer type in tr yet);
// covered by the unit-level smoke check in T-18.a's commit body
// rather than a conformance fixture (which would diverge).

import { writeFile, exists, unlink, appendFile } from 'fs/promises'

await writeFile('/tmp/torajs-async-test.txt', 'hello ')
await appendFile('/tmp/torajs-async-test.txt', 'async fs')

let exists_before: boolean = await exists('/tmp/torajs-async-test.txt')
console.log(exists_before)  // true

await unlink('/tmp/torajs-async-test.txt')

let exists_after: boolean = await exists('/tmp/torajs-async-test.txt')
console.log(exists_after)   // false
