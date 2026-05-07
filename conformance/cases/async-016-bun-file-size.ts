// T-18.c (v0.5.0) — `Bun.file(p).size` synchronous getter (NOT a
// method, NOT async — bun spec). Returns the file's byte size in
// bytes. tr returns -1 for missing / non-regular paths (bun
// returns 0; the -1 sentinel keeps the missing case observable
// until typed-throw fs lands). The fixture explicitly checks
// known-existent file sizes to avoid the divergence.

import { writeFile, unlink } from 'fs/promises'

await writeFile('/tmp/torajs-bun-size-fixture.txt', '0123456789')
console.log(Bun.file('/tmp/torajs-bun-size-fixture.txt').size)  // 10

await writeFile('/tmp/torajs-bun-size-empty.txt', '')
console.log(Bun.file('/tmp/torajs-bun-size-empty.txt').size)    // 0

await writeFile('/tmp/torajs-bun-size-utf8.txt', 'café')
// 'café' is 5 bytes in UTF-8 (é is 2 bytes)
console.log(Bun.file('/tmp/torajs-bun-size-utf8.txt').size)     // 5

await unlink('/tmp/torajs-bun-size-fixture.txt')
await unlink('/tmp/torajs-bun-size-empty.txt')
await unlink('/tmp/torajs-bun-size-utf8.txt')
