// ES §10.1.5.1 [[Set]] on a non-extensible DynObj must reject new
// keys (strict mode; tr modules are all strict). Regression fix:
// __torajs_dynobj_set (implicit-set entry, i.e. `obj.newKey = v`)
// had a writable-gate for existing entries but no extensibility-gate
// for fresh inserts, so `Object.preventExtensions({}); obj.k = 5`
// silently created the entry (matching test262
// Object/preventExtensions/15.2.3.10-3-{2,5-1,6,7,12,15,16,17}.js
// wants a TypeError). Mirrors dynobj_define's fresh-insert gate
// with bun's exact wording.

// core witness: silent-succeed → throw.
{
  const o: any = Object.preventExtensions({});
  let threw: any = null;
  try {
    o.exName = 5;
  } catch (e: any) {
    threw = e;
  }
  console.log("post-assign threw:", threw !== null);
  console.log("threw message:", threw && threw.message);
  console.log("hasOwn exName:", o.hasOwnProperty("exName"));
}

// existing key writes still succeed (not affected by extensibility).
{
  const o: any = { existing: 1 };
  Object.preventExtensions(o);
  o.existing = 42;
  console.log("existing after write:", o.existing);
}

// readonly gate still throws on non-writable existing entries
// (was already correct; regression witness).
{
  const o: any = {};
  Object.defineProperty(o, "ro", { value: 7, writable: false });
  Object.preventExtensions(o);
  let threw: any = null;
  try {
    o.ro = 99;
  } catch (e: any) {
    threw = e;
  }
  console.log("readonly threw:", threw !== null);
  console.log("readonly still:", o.ro);
}

// preventExtensions on an already-populated dict: existing keys
// unchanged, fresh insert rejected.
{
  const o: any = { a: 1, b: 2 };
  Object.preventExtensions(o);
  o.a = 10; // OK
  o.b = 20; // OK
  let threw: any = null;
  try {
    o.c = 30;
  } catch (e: any) {
    threw = e;
  }
  console.log("mixed a:", o.a, "b:", o.b, "c reject:", threw !== null);
}

// isExtensible sees the flag before and after.
{
  const o: any = {};
  console.log("pre isExt:", Object.isExtensible(o));
  Object.preventExtensions(o);
  console.log("post isExt:", Object.isExtensible(o));
}
