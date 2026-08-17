// §16.2.1.6 one-set-of-bindings — a static import's binding must not
// block a dyn candidate for the SAME path (the class is that very
// binding, not a collision); a second candidate with its own class
// rides beside it.
import { Kc } from "./lib_kc.ts";
const d1 = await import("./lib_kc.ts");
const d2 = await import("./lib_j.ts");
console.log(new Kc().p(), new d1.Kc().p(), d1.tag, new d2.J().q());
