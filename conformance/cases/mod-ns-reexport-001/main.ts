// §16.2.3 — a source-ful named re-export (`export { a as b } from
// "m"`) is one of the lib's own exports, so a namespace request —
// static or dynamic — carries the face as a field bound to the
// source module's binding (a synthetic `__reex_<ns>_<face>`; the
// face spelled by the source's own name would collide across libs).
// Covers: cross-file const, self-import var (the FIXTURE shape), and
// a lib whose own default rides beside the re-export.
import * as sns from "./lib_self.ts";
console.log(sns.local1, sns.indirect, sns.default);
import("./lib_hub.ts").then((m: any) => {
  console.log(m.own, m.fromBase);
});
