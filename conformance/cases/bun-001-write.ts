// v0.3 #2: Bun namespace (minimum) — Bun.write aliases to the
// fs.writeFileSync intrinsic. Bun.file (chained-method shape
// returning a File object) lands when surface gains object-result
// Calls.

import { readFileSync, existsSync } from "fs";

const tmp = "/tmp/torajs_conf_bun.txt";
Bun.write(tmp, "hello-bun-from-conformance");
console.log(existsSync(tmp));
console.log(readFileSync(tmp).length);

Bun.write(tmp, "x");
console.log(readFileSync(tmp).length);
