// §16.2.1.5 — every request shape evaluates the module body once:
// named (with a non-exported helper only the body reads), default,
// namespace, an aliased name, and a later side-effect request of a
// module a named request already evaluated.
import { Z } from "./helper.ts";
import v from "./d.ts";
import * as ns from "./n.ts";
import { f as g } from "./se.ts";
import "./se.ts";
console.log(Z, v, ns.Q, g());
