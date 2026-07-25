// `Promise.resolve(undefined)` settles with the same thing
// `Promise.resolve()` does (§27.2.4.7).
//
// The zero-argument form was right all along: it allocates a
// fulfilled promise carrying nothing and stamps the void repr. The
// one-argument form took the ordinary value path instead — an
// undefined-typed operand is not heap-shaped, so it reached the
// primitive allocator WITHOUT that stamp, while the type side called
// the result `Promise<any>` and so decoded the read as a pointer. The
// two halves disagreed about what was in there, and the reader won:
// `[unknown-any-tag]`, with `typeof` answering "object".

async function main() {
  const a = await Promise.resolve(undefined);
  console.log(a);
  console.log(typeof a);
  console.log(a === undefined);

  // through a binding, and with the trailing args the spec drops
  const u = undefined;
  console.log(await Promise.resolve(u));
  console.log(await Promise.resolve(undefined, 1, 2));

  // the zero-arg form it now agrees with
  const z = await Promise.resolve();
  console.log(z);
  console.log(typeof z);

  // ordinary values are untouched
  console.log(await Promise.resolve(42));
  console.log(await Promise.resolve("s"));
  console.log(await Promise.resolve([1, 2]));

  // and it still absorbs a promise
  console.log(await Promise.resolve(Promise.resolve(7)));

  // .then sees it too, in both callback shapes — the spec hands the
  // settled value to the callback, and rejecting that shape had made
  // `Promise.resolve().then((v) => …)` a type error even though bun
  // runs it
  const t = await Promise.resolve(undefined).then((v) => typeof v);
  console.log(t);
  const t0 = await Promise.resolve(undefined).then(() => "no arg");
  console.log(t0);
  const tz = await Promise.resolve().then((v) => typeof v);
  console.log(tz);
}

main();
