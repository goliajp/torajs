// `var A = class {}` binds a class VALUE to a user name. `new A()` and
// `A.m()` already resolve that name to the synth class the parser
// minted; the `extends` position did not, so field flattening looked
// `A` up among declared class NAMES, found nothing, and rejected the
// whole program as a forward reference — even though A was bound above.
const Base = class {
  tag = "base";
  greet() {
    return "hi " + this.tag;
  }
};
const Derived = class extends Base {
  extra = 7;
};
const d = new Derived();
console.log(d.tag);
console.log(d.extra);
console.log(d.greet());
console.log(d instanceof Base);

// a named class declaration may extend such a binding too
class Named extends Base {
  n = 1;
}
const nn = new Named();
console.log(nn.greet());
console.log(nn.n);
console.log(nn instanceof Base);

// let / var binding forms
let LetBase = class {
  v = "L";
};
let LetSub = class extends LetBase {};
console.log(new LetSub().v);

var VarBase = class {
  v = "V";
};
var VarSub = class extends VarBase {};
console.log(new VarSub().v);

// alias chain: the binding is itself an alias of a class-expr binding
const Alias = Base;
class ViaAlias extends Alias {
  z = 3;
}
const va = new ViaAlias();
console.log(va.greet());
console.log(va.z);

// a plain class declaration parent is unaffected
class Plain {
  p = "P";
}
class PlainSub extends Plain {}
console.log(new PlainSub().p);

// super() through a class-expression parent
const Ctor = class {
  made: string = "";
  constructor() {
    this.made = "yes";
  }
};
class CtorSub extends Ctor {
  also: string = "";
  constructor() {
    super();
    this.also = "ok";
  }
}
const cs = new CtorSub();
console.log(cs.made);
console.log(cs.also);
