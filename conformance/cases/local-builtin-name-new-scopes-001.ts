// A local named after a builtin is what `new` reaches inside every
// scope that binds it: a switch's case block, a loop head, a for-of
// variable, and the bodies of a class declared in its scope.

function inSwitch(k: number) {
  switch (k) {
    case 1:
      const TypeError = function () {};
      console.log(new (TypeError as any)().constructor === TypeError);
      break;
    default:
      console.log(new RangeError("d") instanceof Error);
  }
}

function sharedCaseBlock(k: number) {
  switch (k) {
    case 0:
      let Error: any = null;
    case 1:
      Error = function () {};
      console.log(new (Error as any)().constructor === Error);
  }
}

function forHead() {
  for (let RangeError = function () {}, i = 0; i < 1; i++) {
    console.log(new (RangeError as any)().constructor === RangeError);
  }
  const e = new RangeError("after");
  console.log(e instanceof Error, e.message);
}

function forOf() {
  for (const TypeError of [function () {}]) {
    console.log(new (TypeError as any)().constructor === TypeError);
  }
}

function forOfDestructured() {
  for (const { SyntaxError } of [{ SyntaxError: function () {} }]) {
    console.log(new (SyntaxError as any)().constructor === SyntaxError);
  }
}

function classMethod() {
  function TypeError() {}
  class K {
    m() {
      return new (TypeError as any)();
    }
    static s() {
      return new (TypeError as any)();
    }
  }
  console.log(new K().m().constructor === TypeError, K.s().constructor === TypeError);
}

async function asyncClassMethod() {
  function TypeError() {}
  class K {
    m() {
      return new (TypeError as any)();
    }
  }
  console.log(new K().m().constructor === TypeError);
}

function classMembers() {
  const Error = function () {};
  class K {
    f = new (Error as any)();
    static sf = new (Error as any)();
    static sb: any;
    static {
      K.sb = new (Error as any)();
    }
    c: boolean;
    constructor() {
      this.c = new (Error as any)().constructor === Error;
    }
  }
  const k = new K();
  console.log(k.f.constructor === Error, k.c, K.sf.constructor === Error, K.sb.constructor === Error);
}

function methodOwnBinding() {
  class K {
    m() {
      const RangeError = function () {};
      return new (RangeError as any)().constructor === RangeError;
    }
  }
  console.log(new K().m(), new RangeError("x") instanceof Error);
}

class Widget {
  tag = 1;
}

class TopLevel {
  m() {
    const Widget = function () {};
    return (new (Widget as any)() as any).tag;
  }
}

inSwitch(1);
inSwitch(2);
sharedCaseBlock(0);
forHead();
forOf();
forOfDestructured();
classMethod();
asyncClassMethod();
classMembers();
methodOwnBinding();
console.log(new TopLevel().m(), new Widget().tag);
