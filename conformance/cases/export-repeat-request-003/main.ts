import { count, total, bump, K, K as K2 } from "./lib";
import { f } from "./lib";
import { f as g } from "./lib";
bump();
bump();
console.log(count, total);
const k = new K2();
console.log(k instanceof K, K2 === K, K2.s);
function outer() { const f = () => 1; return f() + g(); }
console.log(outer(), f(), g(), f === g);
