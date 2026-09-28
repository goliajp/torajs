// a declaration exported under its own name and again through a bare
// clause is one binding under every spelling, across one clause and
// across later requests; a later request for an export no earlier
// request touched still resolves
import { count, total, bump, inc, K, Klass } from "./lib";
import { total as t2 } from "./lib";
bump();
inc();
console.log(count, total, t2, K === Klass, Klass.s);
import { peek } from "./lib";
console.log(peek(), count === total);
