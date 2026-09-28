// a declaration exported under its own name and again through a bare
// clause is one binding under every spelling, across one clause and
// across later requests; a third request for a declaration no earlier
// request landed lands it under its own name
import { count, total, bump, inc, K, Klass } from "./lib";
import { total as t2 } from "./lib";
import { peek } from "./lib";
bump();
inc();
console.log(count, total, t2, K === Klass, Klass.s);
console.log(count === total, peek());
