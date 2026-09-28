// a declaration exported through a bare clause nobody imports still
// has its initializer run when the module evaluates
import { other } from "./lib";
console.log("main", other);
