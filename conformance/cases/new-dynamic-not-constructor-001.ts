// ES §13.3.5.1 step 7 — EvaluateNew throws a TypeError when
// IsConstructor(constructor) is false. An ordinary object method has
// no [[Construct]].
const obj: any = {
  m(): number {
    return 1;
  },
};

try {
  const z = new obj.m();
  console.log("constructed " + String(z));
} catch (e: any) {
  console.log(e instanceof TypeError);
}

const nested: any = { deep: { fn: obj.m } };
try {
  const z = new nested.deep.fn();
  console.log("constructed " + String(z));
} catch (e: any) {
  console.log(e instanceof TypeError);
}
