// An async function whose declared (or inferred) inner type is `any` and
// whose control flow falls off the end settles with `undefined`
// (ES §27.7.5.2 — the implicit tail completion carries no value, so the
// promise resolves with undefined regardless of the return annotation).
//
// tr has two representations of "settled with undefined": a
// `Promise<Undefined>` carrying no value, and an Any box holding
// ANY_UNDEF. `await` decodes against the declared inner type, so the
// tail-safety return has to build the one that type asks for — it used to
// build the first while the awaiter read the second, printing
// `[unknown-any-tag]`.

async function inferred(flag: boolean) {
  if (flag) return 1;
}

async function declaredAny(): Promise<any> {}

async function declaredVoid(): Promise<void> {}

async function main() {
  console.log(await inferred(false));
  console.log(await inferred(true));
  console.log(typeof (await inferred(false)));
  console.log(await declaredAny());
  console.log(await declaredVoid());

  // The value survives a round trip through a variable and a template,
  // not just the direct print path.
  const v = await inferred(false);
  console.log(v);
  console.log(`${v}`);
  console.log(v === undefined);
}

main();
