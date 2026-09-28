// a namespace requested after named requests of the same module reads
// each export's one binding, live, whichever request landed it
import { count } from "./lib";
import { bump } from "./lib";
import { peek } from "./lib";
import * as ns from "./lib";
bump();
console.log(count, peek(), ns.count, ns.peek(), ns.unused(), ns.peek === peek);
bump();
console.log(count, ns.count, ns.peek());
