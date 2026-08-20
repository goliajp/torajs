// RFC 20260820-member-call-route knife 2 -- PrivateSet brand check
// (spec 7.3.32): writing a private field or invoking a private
// setter on a foreign receiver (this re-bound via .call) throws
// TypeError instead of installing an expando; the declared-brand
// path keeps the normal write / setter dispatch.
var C = class {
  #f = 'orig';
  method() {
    let self = this;
    function inner() {
      self.#f = 'written';
      return self.#f;
    }
    return inner();
  }
};
let c = new C();
console.log(c.method());
try {
  console.log(c.method.call({}));
} catch (e) {
  console.log('caught TypeError:', e instanceof TypeError);
}
var D = class {
  #v = 1;
  set #s(x) { this.#v = x; }
  get #g() { return this.#v; }
  method() {
    let self = this;
    function inner() {
      self.#s = 42;
      return self.#g;
    }
    return inner();
  }
};
let d = new D();
console.log(d.method());
try {
  console.log(d.method.call({}));
} catch (e) {
  console.log('caught TypeError 2:', e instanceof TypeError);
}
