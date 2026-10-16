// A [[Set]] of null on another object's `value` changes nothing an
// accessor named `value` answers: only [[Delete]] removes it.
const c: Record<string, any> = {};
c.value = null;
function f(d: any) {
  d.value = null;
}
f({});
const o = {
  get value() {
    return 1;
  },
};
const x = o.value * 2;
console.log(x, c.value);
