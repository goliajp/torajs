// a declaration exported under its own name and again through a bare
// clause is one binding under every spelling, across one clause and
// across later requests
import { count, total, bump, inc, K, Klass } from "./lib";
import { total as t2 } from "./lib";
bump();
inc();
console.log(count, total, t2, K === Klass, Klass.s);
console.log(count === total);
