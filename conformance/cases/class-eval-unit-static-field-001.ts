// K4-2 — a unit class with static fields and static blocks: they run
// where the class is written, once per evaluation, in order, after the
// methods are defined, on that evaluation's class object (§15.7.14).
let log: string[] = [];
function mk(): any {
  log.push("enter");
  class K {
    static a = (log.push("init"), 1);
    static b: any = "b0";
    static m() { return this.a * 10 }
    static {
      log.push("blk");
      (this as any).c = this.a + 1;
    }
    static d = K_d();
  }
  log.push("after");
  return K;
}
function K_d(): number { log.push("d"); return 4 }
const A = mk();
A.a = 9;
const B = mk();
console.log(A.a, B.a, A.b, A.c, B.c, A.d, log.join(","));
console.log(A.m(), B.m(), Object.keys(A).join(","), Object.keys(B).join(","));
A.b = "changed"; B.b = "b1";
console.log(A.b, B.b, A === B, A.m === B.m);
let s = 0;
for (let i = 0; i < 200; i++) { const Q = mk(); s += Q.c; }
console.log(s, log.length);
