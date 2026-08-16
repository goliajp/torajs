// §12.7.6 IdentifierName — reserved words are valid generator-method
// names in object literals (`{ *yield() {} }`, t262
// yield-as-generator-method-binding-identifier).
var obj = {
  *yield() {
    yield 3;
    yield 4;
  },
  *default() {
    yield "d";
  },
};

const it = obj.yield();
console.log(it.next().value, it.next().value, it.next().done);
const it2 = obj.default();
console.log(it2.next().value, it2.next().done);
