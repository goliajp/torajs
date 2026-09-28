// a library's `export default <expr>` evaluates at its position when
// the module evaluates, whether or not anyone imports the default
// (§16.2.1.5); a later default import binds to that one evaluation
import { other } from "./lib";
import { read } from "./second";
console.log("main", other, read());
