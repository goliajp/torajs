// T-19.c (v0.5.0) — `Bun.file(p).exists()` returns Promise<boolean>.
// MVP routes through fs.existsSync; real I/O suspension lands with
// T-16. Combined with T-15.g.6.c's await-result type recovery, the
// inline form `console.log(await Bun.file(p).exists())` prints
// true/false directly.
//
// Bool was the missing inner T from T-15.g.6.c — the runtime helper
// returns int64_t so a narrow-bitcast (TruncI64ToBool) bridges back
// to the i1 print_bool dispatch.

import { writeFile, unlink } from 'fs/promises'

await writeFile('/tmp/torajs-bun-exists-fixture.txt', 'present')

console.log(await Bun.file('/tmp/torajs-bun-exists-fixture.txt').exists())
console.log(await Bun.file('/tmp/no-such-path-xyz-torajs').exists())

let p = '/tmp/torajs-bun-exists-fixture.txt'
console.log(await Bun.file(p).exists())

await unlink('/tmp/torajs-bun-exists-fixture.txt')

console.log(await Bun.file('/tmp/torajs-bun-exists-fixture.txt').exists())
