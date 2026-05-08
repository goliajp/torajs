// Read + write a file via fs/promises. Both helpers live in the
// `fs/promises` subset that ships with v0.5; bun + node both expose
// the same surface, tr's import resolves to a built-in shim
// (no node_modules needed).

import { writeFile, readFile } from 'fs/promises'

await writeFile('/tmp/torajs-playground.txt', 'hello from a file')
let contents = await readFile('/tmp/torajs-playground.txt')
console.log(contents)
