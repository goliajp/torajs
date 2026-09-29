// an indirect-export chain that bounces between two modules under a
// different name at every hop (§16.2.1.6.3 ResolveExport walks the
// resolveSet by (module, export name), so the chain is not a cycle):
// the importer's spelling rides the whole chain while the name asked
// of each module changes, and every hop must be followed
export { a } from "./m.ts";
export { c as b } from "./m.ts";
export { e as d } from "./m.ts";
export var z = 45;
import * as self from "./main.ts";
import { a as fromM } from "./m.ts";
console.log(self.a, self.b, self.d, fromM, Object.keys(self).join(","));
