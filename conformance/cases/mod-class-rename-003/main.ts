import * as ns from "./lib_ns";
class NC {
  v() { return 2; }
}
console.log(ns.jn(), new ns.NC().v(), new NC().v());
console.log(ns.ncName(), ns.NC.name, NC.name);
