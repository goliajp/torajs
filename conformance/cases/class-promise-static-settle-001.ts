// §27.2.4.6/.7 + §15.7.14 — the settle statics inherited through a
// builtin-parent class object run the builtin settle: |this| reaches
// the interned Promise ctor cell through the class-object chain
// (two-hop subclass included); a detached call keeps the TypeError.
class CP extends Promise<any> {}
const cp: any = CP;
cp.resolve(41).then((v: any) => console.log("resolved", v + 1));
cp.reject(new Error("boom")).catch((e: any) => console.log("caught", e.message));
cp.all([cp.resolve(1), 2]).then((xs: any) => console.log("all", xs[0] + xs[1]));

class CP2 extends CP {}
const cp2: any = CP2;
console.log(typeof cp2.resolve);
cp2.resolve("two-hop").then((v: any) => console.log("resolved", v));

const detached = cp.resolve;
try {
  detached(1);
} catch (e: any) {
  console.log("detached threw");
}
