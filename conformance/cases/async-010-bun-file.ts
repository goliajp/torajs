// T-19 (v0.5.0) — `Bun.file(path).text()` reads the file as string,
// wraps in Promise.resolve(...). MVP "synchronous-then-resolve" —
// real I/O suspension lands with T-16 state-machine async/await.
//
// Known limit (T-15.g.6 follow-up): direct `console.log(await
// Bun.file(p).text())` prints the str ptr instead of contents
// because Type::Promise is type-erased at SSA. Storing in
// `let s: string` first restores the type via the LetDecl arm's
// slot-shape coercion. This fixture uses the let-typed pattern
// throughout.
//
// `.json()` and `.arrayBuffer()` deferred until tr's stdlib gains
// the corresponding return types (any-typed JSON parse + Buffer).

import { writeFile, unlink } from 'fs/promises'

await writeFile('/tmp/torajs-bun-file-test.txt', 'hello bun.file')

let s: string = await Bun.file('/tmp/torajs-bun-file-test.txt').text()
console.log(s)

let s2: string = await Bun.file('/tmp/torajs-bun-file-test.txt').text()
console.log(s2 + ' (read again)')

await unlink('/tmp/torajs-bun-file-test.txt')
