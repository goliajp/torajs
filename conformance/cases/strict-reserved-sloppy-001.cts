// §12.7.2 — `implements interface package private protected public
// static` are reserved ONLY in strict code. Sloppy script code binds
// them as ordinary identifiers, and that admission is what this pins:
// the strict half rejects at parse time (per-function, from the
// enclosing directive) or at the goal gate (module code), neither of
// which a byte-comparison fixture can express.
var staticv = 1;
var interfacev = 2;

var static = 10;
var public = 20;
var private = 30;
var protected = 40;
var implements = 50;
var interface = 60;
var package = 70;

function reads() {
  var static = 100;
  return static + public;
}

console.log(static + public + private + protected + implements + interface + package);
console.log(reads());
console.log(staticv + interfacev);
