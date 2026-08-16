// §16.2.1.5 with a cycle: a and b request each other. Post-order has
// to stop at the member already on the stack rather than recurse
// forever, and each member's body still runs exactly once — the
// resolver's visited set does both.
import "./a.ts";
console.log("main");
