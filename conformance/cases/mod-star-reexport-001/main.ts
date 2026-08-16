// §16.2.3 `export * from "m"` — a star re-export preserves names, so
// the importer's request forwards to `m` unchanged. Pre-fix tora died
// in the PARSER (`expected expression, got Star`): `parse_export` had
// no `*` arm and fell through to `export <decl>`, which tried to read
// the `*` as the head of an expression statement.
//
// Substrate fix (r421):
// - ast: ExportDecl gains `star: Option<ExportStar>`
// - parser: `export * from "m"` / `export * as ns from "m"`
// - modules: the star arm forwards the importer's wanted set to `m`,
//   MINUS whatever the hub exports itself (local export shadows the
//   star's — `shared` below)
//
// Coverage: pass-through const / fn / class, a name the hub shadows,
// and the importer's own alias applied across the star (`Ca`).
import { A_VAL, fa, Ca as RenamedC, B_VAL, shared } from "./b.ts";
console.log(A_VAL);
console.log(fa());
console.log(new RenamedC().m());
console.log(B_VAL);
console.log(shared);
