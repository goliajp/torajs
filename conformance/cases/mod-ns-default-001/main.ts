// §16.2.1.10 — the namespace object carries the lib's default export
// as its `default` field: the combined form (d, * as ns), the
// ns-only form (synthetic `__nsdefault_` binding), and a dynamic
// import revisiting a statically-walked path all answer it.
// The `export * as inner` namespace carries its module's default too
// (§16.2.3.7) — while a bare `export * from` pour must NOT forward it
// (§16.2.3, pinned by mod-ns-missing-member-001).
import d, * as ns1 from "./lib_d.ts";
import * as ns2 from "./lib_e.ts";
import * as wrap from "./lib_wrap.ts";
console.log(d, ns1.default, ns2.default, ns2.tag);
console.log(wrap.own, wrap.inner.q, wrap.inner.default);
import("./lib_e.ts").then((m: any) => {
  console.log(m.default, m.tag);
});
