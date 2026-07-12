// Object.create(proto, props) + Object.defineProperties runtime props
// walk — RFC 20260712-object-create-define-props chunk 2
// create with data props (literal)
const o: any = Object.create({}, {
  prop: { value: 42, enumerable: true, writable: false, configurable: false },
});
console.log(o.prop);
console.log(Object.prototype.hasOwnProperty.call(o, "prop"));
const d: any = Object.getOwnPropertyDescriptor(o, "prop");
console.log(d.value, d.writable, d.enumerable, d.configurable);
// empty descriptor -> own prop, value undefined
const o2: any = Object.create({}, { p: {} });
console.log(Object.prototype.hasOwnProperty.call(o2, "p"), o2.p);
// accessor in props
let accessed = false;
const o3: any = Object.create({}, {
  g: { get: () => { accessed = true; return 7; }, enumerable: true },
});
console.log(o3.g, accessed);
// multiple props + enumerable filter
const o4: any = Object.create({}, {
  a: { value: 1, enumerable: true },
  b: { value: 2, enumerable: false },
  c: { value: 3, enumerable: true },
});
for (const k in o4) console.log("k", k, o4[k]);
console.log(Object.keys(o4).length);
// runtime props variable for create
const propsVar: any = { rp: { value: 9, enumerable: true, writable: true, configurable: true } };
const o5: any = Object.create({}, propsVar);
console.log(o5.rp);
// defineProperties runtime props
const o6: any = {};
Object.defineProperties(o6, propsVar);
console.log(o6.rp, Object.prototype.hasOwnProperty.call(o6, "rp"));
// defineProperties runtime accessor descriptor
const propsAcc: any = { g2: { get: () => 11 } };
Object.defineProperties(o6, propsAcc);
console.log(o6.g2);
// non-object descriptor value -> TypeError
const tgt: any = {};
const badProps: any = { x: 5 };
try {
  Object.defineProperties(tgt, badProps);
  console.log("no-throw");
} catch (e) {
  console.log("bad-desc", e instanceof TypeError);
}
// mid-walk non-configurable redefine throws, prior state kept
const o8: any = {};
Object.defineProperty(o8, "nc", { value: 1, configurable: false });
const conflicting: any = { nc: { value: 2 } };
try {
  Object.defineProperties(o8, conflicting);
  console.log("no-throw");
} catch (e) {
  console.log("mid-throw", e instanceof TypeError, o8.nc);
}
// repeated create+props in a loop (temp release lane)
for (let i = 0; i < 3; i++) {
  const c: any = Object.create({}, { v: { value: i, enumerable: true } });
  console.log("loop", c.v);
}
