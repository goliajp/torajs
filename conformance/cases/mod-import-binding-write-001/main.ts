// §16.2.1.5 CreateImportBinding — the importer's view is immutable,
// but the EXPORTING module writing its own binding is the legal
// live-binding path and the reader sees the update. Pins that the
// import-binding write rejection (entry-arena rewrite) does not
// reach lib-side self-writes. The entry-side illegal write
// (`v = 99` → runtime TypeError) is exercised by test262
// instn-iee-bndng-*; bun compile-rejects that form, so it can't
// ride a bun-parity fixture.
import { v, bump } from "./lib.ts";
console.log(v);
bump();
console.log(v);
