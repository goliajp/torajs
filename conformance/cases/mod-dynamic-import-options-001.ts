// §13.3.10 ImportCall's optional second argument (import options)
// plus the grammar-legal trailing comma in both arg forms. Pre-fix
// the parser demanded `)` right after the source string literal.

const m: any = await import("./mod-dynamic-import-options-001-lib.ts", {});
console.log(m.twice(4));
const n: any = await import("./mod-dynamic-import-options-001-lib.ts",);
console.log(n.marker);
