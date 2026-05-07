// T-19.d (v0.5.0) — `Bun.file(p).json()` with caller-driven typing.
// The LetDecl arm detects the `await Bun.file(p).json()` chain
// (after the parser's `await e` → `e.value` desugar) and routes
// through the same JSON parser machinery as `JSON.parse(text)`,
// using the slot's concrete T (Struct / number / string / array)
// to drive parsing.
//
// Boolean arrays inside JSON deferred — JSON.parse pre-existing
// arr_push(i64, i1) ABI mismatch when target is `boolean[]`. Other
// shapes (string / number / nested struct / number[]) work.

import { writeFile, unlink } from 'fs/promises'

await writeFile(
  '/tmp/torajs-bun-json-fixture.json',
  '{"name":"torajs","version":42,"counts":[10,20,30]}'
)

type Pkg = { name: string, version: number, counts: number[] }
let pkg: Pkg = await Bun.file('/tmp/torajs-bun-json-fixture.json').json()
console.log(pkg.name)              // torajs
console.log(pkg.version)           // 42
console.log(pkg.counts.length)     // 3
console.log(pkg.counts[0])         // 10
console.log(pkg.counts[2])         // 30

// Number-only json file
await writeFile('/tmp/torajs-bun-json-num.json', '99')
let n: number = await Bun.file('/tmp/torajs-bun-json-num.json').json()
console.log(n)                     // 99

await unlink('/tmp/torajs-bun-json-fixture.json')
await unlink('/tmp/torajs-bun-json-num.json')
