// evaluating a module runs every export's initializer, whether or not
// the importer asked for that export (§16.2.1.5); a colliding name in
// the importer does not stop it
import { other } from "./lib";
const r = "mine";
console.log("main", other, r);
