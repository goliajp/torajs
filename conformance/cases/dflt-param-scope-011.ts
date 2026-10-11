// a parameter default cannot see the body's declarations (ECMA-262
// §10.2.11 gives the body its own environment), so a body binding with
// the same name neither hides the outer one's dead zone nor answers the
// default once the outer one is initialized
function f(y = K) {
  let K = 2;
  return y + K;
}
const g = (y = K) => {
  const K = 3;
  return y + K;
};
try {
  f();
} catch (e) {
  console.log("f early:", e instanceof ReferenceError);
}
try {
  g();
} catch (e) {
  console.log("g early:", e instanceof ReferenceError);
}

const fexpr = function (y = K) {
  var K = 4;
  return y + K;
};
function withFn(y = K) {
  function K() {
    return 9;
  }
  return typeof y + ":" + K() + ":" + K.name;
}
function withClass(y = K) {
  class K {}
  return y + typeof K;
}
class C {
  m(y = K) {
    let K = 5;
    return y + K;
  }
  static s(y = K) {
    const K = 6;
    return y + K;
  }
  *gen(y = K) {
    let K = 7;
    yield y + K;
  }
  async am(y = K) {
    let K = 8;
    return y + K;
  }
}
class D {
  v: number;
  constructor(y = K) {
    const K = 7;
    this.v = y + K;
  }
}
function* gen(y = K) {
  let K = 3;
  yield y + K;
}
async function af(y = K) {
  let K = 4;
  return y + K;
}
const aa = async (y = K) => {
  let K = 6;
  return y + K;
};
const o = {
  m(y = K) {
    let K = 2;
    return y + K;
  },
};
function outer() {
  function inner(y = K) {
    let K = 2;
    return y + K;
  }
  return inner();
}
function viaClosure(get = () => K) {
  let K = 20;
  return get() + K;
}
function bodyClosure(y = K) {
  let K = 2;
  const r = () => K;
  return y + r();
}
function typed(y: number = K) {
  let K = 2;
  return y + K;
}
function bodyDeadZone(y = K) {
  try {
    const z = K;
    return "read " + z;
  } catch (e) {
    return "dead zone " + (e instanceof ReferenceError);
  }
  let K = 2;
}

const K = 1;
console.log("f g:", f(), g(), f(10), g(10));
console.log("function expression:", fexpr());
console.log("body function:", withFn());
console.log("body class:", withClass());
console.log("method:", new C().m(), C.s(), new C().gen().next().value);
console.log("constructor:", new D().v);
console.log("generator:", gen().next().value);
console.log("object method:", o.m());
console.log("nested:", outer());
console.log("closure default:", viaClosure());
console.log("body closure:", bodyClosure());
console.log("typed:", typed());
console.log("body dead zone:", bodyDeadZone());
af().then((v) => console.log("async function:", v));
aa().then((v) => console.log("async arrow:", v));
new C().am().then((v) => console.log("async method:", v));
