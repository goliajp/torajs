// The request sits BETWEEN two statements, so a reader can tell the
// difference between "runs first" and "runs where it is written":
// §16.2.1.5 evaluates leaf before any of this body.
console.log("mid before");
import "./leaf.ts";
console.log("mid after");
