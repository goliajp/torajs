// §15.7.1 early-error guards — the LEGAL faces the new rejects must not hit:
// computed ["constructor"] field (PropName of ComputedPropertyName is empty),
// `arguments` as a member NAME (not an IdentifierReference), the string-keyed
// 'constructor'() method which IS the constructor, and the async-modifier
// lookahead admitting a computed member name.
class C {
  ["constructor"] = 1;
}
const c: any = new C();
console.log(c["constructor"]);

const obj = { arguments: 7 };
class D {
  y = obj.arguments;
}
console.log(new D().y);

class E {
  "constructor"() {
    console.log("ctor ran");
  }
}
new E();

class F {
  async ["m"]() {
    return 4;
  }
}
(new F() as any).m().then((v: any) => console.log(v));
