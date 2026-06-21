// ES §27.2.4.{7,8} — Promise.{resolve,reject}(value, ...trailing)
// trailing-arg ignore: spec evaluates each trailing arg left-to-right
// and discards. Prior S263 widened typecheck + ssa-emit gate (fixture
// 001 covered literal trailing), but SSA-emit body only referenced
// args[0] — trailing args were lowered nowhere = silent-drop of
// side-effect exprs. step()-counter probe exposes the gap: bun fires
// all step() prints, tora silent-drops them. S322 adds the
// lower-and-drop loop after args[0] lower / before arg_ty dispatch.

function step(label: string): number {
  console.log(label);
  return 0;
}

const p1 = Promise.resolve(42, step("t1") as any);
console.log("p1=", typeof p1);

const p2 = Promise.resolve("hello", step("t2") as any, step("t3") as any);
console.log("p2=", typeof p2);

const p3 = Promise.resolve(true, step("t4") as any);
console.log("p3=", typeof p3);

const p4 = Promise.reject(99, step("t5") as any);
p4.then((_v: number): number => 0, (_r: number): number => 0);
console.log("p4=", typeof p4);

const p5 = Promise.reject(7, step("t6") as any, step("t7") as any);
p5.then((_v: number): number => 0, (_r: number): number => 0);
console.log("p5=", typeof p5);
