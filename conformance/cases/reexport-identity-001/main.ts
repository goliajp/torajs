// a binding reached directly, through a namespace and through a
// re-exporting module is one binding; the entry's own import of the
// re-exported spelling reserves nothing against the module it
// resolves to
import { a } from "./lib";
import * as m from "./lib";
import { a as a2, f } from "./mid";
console.log(a === m.a, a === a2, f === m.f, a2.k, f());
