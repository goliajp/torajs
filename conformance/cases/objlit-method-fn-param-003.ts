// A class method and an object-literal method share a name; only the
// object-literal one receives arrows through its member calls. A
// function forwarding its fn-typed parameter to either, a function
// handed on as a callback, and a method value read off the object
// all keep calling the right function.
class C {
  k(cb: (v: number) => void) {
    cb(10);
  }
}
const o = {
  k(cb: (v: number) => void) {
    cb(20);
  },
  plain(cb: (v: number) => void) {
    cb(30);
  },
};
function named(v: number) {
  console.log("named", v);
}
new C().k(named);
o.k(named);
o.k((v: number) => console.log("o arrow", v));
o.plain(named);
function viaC(cb: (v: number) => void) {
  new C().k(cb);
}
function viaO(cb: (v: number) => void) {
  o.k(cb);
}
viaC(named);
viaO(named);
viaO((v: number) => console.log("relayed arrow", v));
const p = {
  k(cb: (v: number) => void) {
    cb(0);
  },
};
function fwd(cb: (v: number) => void) {
  p.k(cb);
}
function run(cb: (v: number) => void) {
  cb(1);
}
run(named);
fwd(named);
const q = {
  m(cb: (v: number) => void) {
    cb(40);
  },
};
const m = q.m;
m((v: number) => console.log("method value", v));
function nested(cb: (v: number) => void) {
  [1].forEach(() => o.k(cb));
}
nested(named);
