// §27.2.4.7 step 2 (PromiseResolve steps 1-2) on the inherited
// resolve: an argument that is already a C instance answers by
// identity; a DIFFERENT class's instance (or a plain Promise) still
// mints a fresh C promise that adopts it.
class CP extends Promise<any> {}
const cp: any = CP;
const inst = cp.resolve(1);
console.log(cp.resolve(inst) === inst);
class CP2 extends CP {}
console.log((CP2 as any).resolve(inst) === inst);
const plain = Promise.resolve(2);
console.log(cp.resolve(plain) === plain);
cp.resolve(inst).then((v: any) => console.log("v", v));
