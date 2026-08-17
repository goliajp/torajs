// §16.2.1.10 — the namespace object carries the lib's default export
// as its `default` field: the combined form (d, * as ns), the
// ns-only form (synthetic `__nsdefault_` binding), and a dynamic
// import revisiting a statically-walked path all answer it.
import d, * as ns1 from "./lib_d.ts";
import * as ns2 from "./lib_e.ts";
console.log(d, ns1.default, ns2.default, ns2.tag);
import("./lib_e.ts").then((m: any) => {
  console.log(m.default, m.tag);
});
