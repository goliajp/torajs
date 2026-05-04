// v0.3 #1: fs module — readFileSync / writeFileSync / existsSync.
// `import { ... } from "fs"` is recognized at desugar time and the
// imported names rewritten to `fs.<method>` member access; same
// dispatch as direct `fs.readFileSync(...)` calls.
//
// Note: bun's `readFileSync(path)` (no encoding) returns Buffer;
// tr returns string. The conformance assertions here use only the
// shapes that agree (existsSync boolean, .length on both string and
// Buffer match). The conformance runner re-runs the case 3 times on
// the same temp path (bun → jit → aot), so we don't assert on
// "file did not exist before" — only on round-trip behavior.

import { readFileSync, writeFileSync, existsSync } from "fs";

const tmp = "/tmp/torajs_conf_fs.txt";

writeFileSync(tmp, "round-trip");
console.log(existsSync(tmp));
console.log(readFileSync(tmp).length);

writeFileSync(tmp, "x");
console.log(readFileSync(tmp).length);
writeFileSync(tmp, "");
console.log(readFileSync(tmp).length);
writeFileSync(tmp, "abcdefghij");
console.log(readFileSync(tmp).length);
