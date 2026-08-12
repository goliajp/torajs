// r380 — §12.7.2's strict reserved words at the CLASS NAME position
// (L3b 376-04's class-name half). §15.7 makes every part of a class
// strict whatever the goal said, so these are SyntaxErrors in a
// sloppy script too -- which is what this file being `.cts` tests.
// tr used to take all seven.
//
// The rejects themselves cannot be written here (they are parse
// errors); what this fixture pins is that the words stay usable
// everywhere the spec still allows them in sloppy code, so the new
// judge did not spill past the class-name position.

var pkg = 1;
var priv = 2;

// still ordinary identifiers in sloppy code
var package_ok = (function () {
  var implements_ = 1;
  return implements_;
})();
console.log(package_ok);

function statik(a) { return a; }
console.log(statik(3));

// and the class-name position still takes every ordinary name
class Interface { v = 4; get() { return this.v; } }
console.log(new Interface().get());

class Public { static make() { return 5; } }
console.log(Public.make());

const Named = class Inner { n = 6; get() { return this.n; } };
console.log(new Named().get());

const Anon = class { n = 7; get() { return this.n; } };
console.log(new Anon().get());

// a member named with one of the words is unaffected -- property
// names are not identifier references
const holder = { static: 8, public: 9, private: 10 };
console.log(holder.static, holder.public, holder.private);

class WithMembers {
  static field = 11;
  get public() { return 12; }
}
console.log(WithMembers.field, new WithMembers().public);
