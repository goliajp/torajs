// T-18.b (v0.5.0) — fs.readdir async + readdirSync. Returns
// Array<string> with one entry per child (`.` and `..` filtered
// per spec). Order matches the OS's readdir(); the fixture checks
// MEMBERSHIP rather than order to stay portable across FS impls.

import { existsSync, mkdirSync } from 'fs'
import { readdir, writeFile, unlink } from 'fs/promises'

// Idempotent setup so re-runs don't EEXIST. mkdirSync only fires
// when the dir is missing; the inner files get re-written each run.
let dir: string = '/tmp/torajs-readdir-fixture'
if (!existsSync(dir)) {
  mkdirSync(dir)
}

await writeFile(dir + '/aa.txt', '1')
await writeFile(dir + '/bb.txt', '2')

let entries: string[] = await readdir(dir)
console.log(entries.length >= 2)  // true (at least the 2 we just wrote)

let saw_a: boolean = false
let saw_b: boolean = false
for (let i: number = 0; i < entries.length; i = i + 1) {
  if (entries[i] === 'aa.txt') saw_a = true
  if (entries[i] === 'bb.txt') saw_b = true
}
console.log(saw_a)  // true
console.log(saw_b)  // true

await unlink(dir + '/aa.txt')
await unlink(dir + '/bb.txt')
