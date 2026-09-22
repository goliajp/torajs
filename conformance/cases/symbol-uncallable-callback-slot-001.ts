// §7.2.3 IsCallable(Symbol) is false: a Symbol in a callback slot is the
// spec's runtime TypeError (after the arguments evaluate), and so is
// calling one — the same band as a number or a RegExp there.
const log: string[] = [];
try { [2, 1].sort(Symbol("c")); } catch (e) { console.log("sort", (e as any).constructor.name); }
try { [2, 1].find(Symbol()); } catch (e) { console.log("find", (e as any).constructor.name); }
try { [2, 1].map((log.push("arg"), Symbol())); } catch (e) { console.log("map", (e as any).constructor.name, log.join()); }
const s = Symbol("f");
try { (s as any)(); } catch (e) { console.log("call-any", (e as any).constructor.name); }
try { s(); } catch (e) { console.log("call", (e as any).constructor.name); }
