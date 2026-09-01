// ShortStr values through the emit-side any lanes: default sort's
// ToString comparison over Array<Any> and `??` unboxing into a typed
// binding keep value semantics; churn probes assert the
// materialization-leak side (546-02 core-emit batch).
const xs: any[] = ["cd", "ab", "ef", "b"];
xs.sort();
console.log(JSON.stringify(xs));
const s: any = "ab";
const t: string = s ?? "zz";
console.log(t);
const u: string = (null as any) ?? "zz";
console.log(u);
