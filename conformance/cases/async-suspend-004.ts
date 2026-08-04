// RFC 20260805 blade 1 — the driver carries both directions: a value
// sent back into the body, a rejection delivered to the body's catch,
// and a throw escaping the body as a rejection of the returned promise.
async function ok(): Promise<any> {
  const v: any = await Promise.resolve(7);
  return v + 1;
}
async function caught(): Promise<any> {
  try {
    await Promise.reject("boom");
  } catch (e: any) {
    return "caught:" + e;
  }
  return "nope";
}
async function escapes(): Promise<any> {
  await Promise.resolve(1);
  throw "escaped";
}
ok().then((v: any) => { console.log("ok", v); return 0; });
caught().then((v: any) => { console.log("caught", v); return 0; });
escapes().catch((e: any) => { console.log("escapes", e); return 0; });
console.log("sync-end");
