// §16.2.1.5.3 — namespace and default requests each evaluate their
// module's body, dependencies first; a module two requests reach (a.ts,
// through main's namespace import and b.ts's named import) runs once.
import * as a from "./a.ts";
import items from "./b.ts";
console.log("main " + a.value() + " " + items.length + " " + items[0]);
