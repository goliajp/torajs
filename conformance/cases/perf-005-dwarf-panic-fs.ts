// v0.3 #4 D-6 — DWARF panic backtrace fixture. The bench-harness
// runs every fixture through `bun` first to capture the oracle
// stdout; bun throws on `readFileSync` of a missing path, exiting
// non-zero. The conformance runner skips cases where bun exits
// non-zero (treats them as "negative tests"), so this fixture
// won't gate-fail. It exists so a developer can run `tr build` +
// execute the binary and eyeball that the panic message includes
// `dwarf_panic_*.ts:<line>` resolving the user's failing call.
//
// Manual smoke-test:
//   tr build conformance/cases/perf-005-dwarf-panic-fs.ts -o /tmp/x
//   /tmp/x
//
// Expected output shape (line numbers may shift if this file is edited):
//   not yet supported: fs.readFileSync open failed: /no/such/...
//   backtrace:
//   __torajs_fs_read_file_sync (in x) + ...
//   main (in x) (perf-005-dwarf-panic-fs.ts:N)

import { readFileSync } from "fs";
let s: string = readFileSync("/no/such/path/dwarf_probe.xyz");
console.log(s);
