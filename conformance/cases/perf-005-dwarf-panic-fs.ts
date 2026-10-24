// v0.3 #4 D-6 — DWARF panic backtrace fixture. bun throws on
// `readFileSync` of a missing path and exits 1 with nothing on stdout;
// tr must end the same way. It also exists so a developer can run
// `tr build` + execute the binary and eyeball that the panic message
// includes `dwarf_panic_*.ts:<line>` resolving the user's failing call.
//
// exit-code: 1
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
