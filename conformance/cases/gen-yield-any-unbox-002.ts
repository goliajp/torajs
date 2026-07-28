function* strGen(): string {
  const s: any = "boxed";
  yield s;
}
for (const v of strGen()) console.log(v);
