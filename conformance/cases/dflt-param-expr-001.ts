// RFC 20260729-fn-value-any V2b — expression parameter defaults
// materialize in the callee body (§9.2: evaluated in the callee's
// scope, in parameter order, whenever the bound argument is
// undefined). Covers ref-prior chains, an explicit undefined
// triggering the default, two same-name classes with their own
// defaults, and a detached generator method driven bare through the
// runtime dispatch (the t262 dflt-params-ref-prior template shape).
function h(x: any, y: any = x, z: any = y) {
  console.log("h", x, y, z);
}
h(7);
h(1, 2);
h(1, undefined, 3);

class C {
  m(x: any, y: any = x) {
    console.log("C", y);
  }
}
class D {
  m(x: any, y: any = x * 2) {
    console.log("D", y);
  }
}
new C().m(9);
new D().m(9);

let E = class {
  async *method(x: any, y: any = x, z: any = y) {
    console.log("E", x, y, z);
    yield z;
  }
};
let ref = E.prototype.method;
const g: any = ref(3);
async function main() {
  for await (const v of g) console.log("v", v);
}
main();
