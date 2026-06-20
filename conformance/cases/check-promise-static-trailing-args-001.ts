// S273 — Promise.{all,race,any,allSettled}(iterable, ...trailing) per
// ES §27.2.4.{1,3,5,2}: spec reads only the iterable at args[0];
// trailing slots silent-drop. tora's check.rs previously enforced
// `args.len() != 1 → Err` ("Promise.{m} expects 1 arg ..."); SSA-emit
// guarded `args.len() == 1`.
//
// Fixture avoids printing the allSettled result struct (pre-existing
// Array<Struct> display gap in tora's console.log) and instead checks
// `.length` + `.status` field access to verify the spec shape.

let calls = 0;
function step<T>(v: T): T {
  calls = calls + 1;
  return v;
}

async function main() {
  // 1-arg baseline.
  const a1 = await Promise.all([Promise.resolve(1), Promise.resolve(2)]);
  console.log(a1);
  const r1 = await Promise.race([Promise.resolve("a")]);
  console.log(r1);
  const y1 = await Promise.any([Promise.resolve(1)]);
  console.log(y1);
  const s1 = await Promise.allSettled([Promise.resolve(1)]);
  console.log(s1.length, s1[0].status, s1[0].value);

  // 2-arg trailing — pre-S273 rejected.
  const a2 = await Promise.all([Promise.resolve(3), Promise.resolve(4)], step("a"));
  console.log(a2);
  const r2 = await Promise.race([Promise.resolve("b")], step("b"));
  console.log(r2);
  const y2 = await Promise.any([Promise.resolve(2)], step("c"));
  console.log(y2);
  const s2 = await Promise.allSettled([Promise.resolve(2)], step("d"));
  console.log(s2.length, s2[0].status, s2[0].value);

  // 3+ arg trailing — pre-S273 rejected.
  const a3 = await Promise.all([Promise.resolve(5)], step("e"), step("f"), step("g"));
  console.log(a3);
  const r3 = await Promise.race([Promise.resolve("c")], step("h"), step("i"));
  console.log(r3);
  const y3 = await Promise.any([Promise.resolve(3)], step("j"), step("k"), step("l"));
  console.log(y3);
  const s3 = await Promise.allSettled([Promise.resolve(3)], step("m"), step("n"));
  console.log(s3.length, s3[0].status, s3[0].value);

  console.log("calls=" + calls);
}
main();
