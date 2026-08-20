// RFC 20260820-member-call-route — the t262 private-*-access-on-
// inner-* family shape: `c.method.call({})` re-binds this to a
// foreign object; the inner fn's `self.#f` read must throw
// TypeError (§7.3.31 PrivateGet brand check — the priv-tag member
// channel), never answer undefined.
var C = class {
  #f = 'Test262';
  method() {
    let self = this;
    function inner() {
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
