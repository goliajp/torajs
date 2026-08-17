// 423-01 knife A — per-module deconflict: two modules exporting the
// same name no longer collide in the flat top level. Colliding,
// unrequested decls mangle to __m<k>_<name> (references follow via
// the lib arena slice; async marks follow via the name-keyed
// tables), the namespace object's FIELDS keep the export spelling,
// `.name` strips the mangle, and a named request keeps its user
// spelling (entry-level import bindings reserve theirs up front, so
// BFS order does not decide who keeps a name).
import * as A from "./m1";
import * as B from "./m2";
import { local1 } from "./m3";
import "./se1";
const mine: number = 99;
console.log(A.local1, B.local1, local1, mine);
console.log(A.tag(), B.tag());
console.log(A.tag.name, B.tag.name);
import * as C from "./m3";
C.work().then((v) => console.log("async:", v));
