import { mkB, hasX } from "./lib_brand";
class B {
  #x = 5;
  probe(o: any) { return #x in o; }
}
const eb = new B();
console.log(hasX(mkB()), hasX(eb), eb.probe(mkB()));
console.log(new B().probe(eb), hasX(new B()));
