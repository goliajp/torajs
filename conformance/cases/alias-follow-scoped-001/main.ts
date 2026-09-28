// a second spelling of an export follows the export's binding at every
// site that resolves to the top level, while a parameter or an inner
// declaration of either spelling keeps its own scope
import { n, bump } from "./lib";
import { m, inc } from "./lib";
const twice = (m: number) => m * 2;
function shadowFirst(n: number) { return n * 3; }
function readsLive() { return m; }
function innerLet() { let m = 42; return m; }
bump();
inc();
console.log(n, m, twice(5), shadowFirst(7), readsLive(), innerLet());
