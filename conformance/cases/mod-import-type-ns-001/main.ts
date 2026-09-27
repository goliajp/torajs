// `import type * as ns` and `import type X from` request nothing.
import type * as ns from "./lib.ts";
import type D from "./lib.ts";
console.log("main");
