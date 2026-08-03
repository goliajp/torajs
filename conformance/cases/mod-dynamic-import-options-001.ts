// §13.3.10 ImportCall's optional second argument (import options)
// plus the grammar-legal trailing comma in both arg forms. Pre-fix
// the parser demanded `)` right after the source string literal.
//
// Substrate fix (rotation 289): parse_primary_dyn_import accepts
// `, <expr> ,opt` after the source and DISCARDS the options — the
// eager AOT subset resolves the module at compile time, so options
// cannot change linking (their evaluation side effects are a
// recorded subset boundary).

const m: any = await import("./mod-dynamic-import-options-001-lib.ts", {});
console.log(m.twice(4));
const n: any = await import("./mod-dynamic-import-options-001-lib.ts",);
console.log(n.marker);
