// RFC 20260820-member-call-route knife 3 -- OrdinaryCallBindThis
// puts no shape bound on the thisArgument: a primitive or nullish
// thisValue hosts the receiver-polymorphic twin body, and the spec
// TypeError fires at the member read/write site (GetValue /
// PrivateSet brand), observable by a body-side try. A this-free
// body runs regardless.
class C {
  #p = 1;
  method() {
    try { this.#p = 2; return 'wrote'; }
    catch (e) { return 'caught ' + (e instanceof TypeError); }
  }
  noThis() { return 'ran'; }
}
const c = new C();
console.log(c.method());
console.log(c.method.call(15));
console.log(c.method.call(false));
const bare = c.method;
try { console.log(bare()); } catch (e) { console.log('bare TypeError', e instanceof TypeError); }
try { console.log(c.method.call(null)); } catch (e) { console.log('null TypeError', e instanceof TypeError); }
console.log(c.noThis.call(15));
