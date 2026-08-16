// §13.3.10 — a specifier that resolves to no module rejects at
// RUNTIME (never a compile error). The error message differs between
// hosts, so only the rejection itself is asserted.
import("./THIS_FILE_DOES_NOT_EXIST.js").catch(() => {
  console.log("miss rejected");
});
const dynamic: any = { toString() { return "./also-missing.js"; } };
import(dynamic).catch(() => {
  console.log("dynamic miss rejected");
});
