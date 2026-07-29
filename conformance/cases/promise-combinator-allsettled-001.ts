// RFC 20260730 follow-up — Promise.allSettled any-lane sibling: an
// Array<Any> input (mixed promise / plain elements) builds
// {status, value} settled structs per §27.2.4.3 resolve-wrap, and
// the dyn entry joins the collect-then-delegate shape (strings and
// other iterables settle per element; non-iterables answer a
// rejected TypeError — held by promise-combinator-noniterable-001).
//
// Bun-parity scope: fulfilled entries agree byte-identically
// (status + value); a rejected entry's status agrees while its
// reason slot diverges (bun spec-strict `.reason`, tr MVP `.value`)
// so only the status prints (async-017 posture).

async function main(): Promise<void> {
  // Array<Any> mixed — typed {status, value: any} struct face.
  const xs = [Promise.resolve(1), 2, Promise.reject("bad"), Promise.resolve("s")];
  const rs = await Promise.allSettled(xs);
  console.log(rs.length);
  console.log(rs[0].status, rs[0].value);
  console.log(rs[1].status, rs[1].value);
  console.log(rs[2].status);
  console.log(rs[3].status, rs[3].value);

  // dyn face — a string is spec-iterable, one entry per character.
  const ss: any = await Promise.allSettled("ab");
  console.log("str:" + ss.length);
  console.log("done");
}
main();
