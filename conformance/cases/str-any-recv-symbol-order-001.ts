const s: any = "abab";
const re: any = /a/;
let called = 0;
re[Symbol.matchAll] = function () { called++; return "custom"; };
re[Symbol.replace] = function (str: string, r: any) { called++; return "R:" + str + ":" + r; };
try { s.matchAll(re); console.log("no throw"); } catch (e: any) { console.log(e instanceof TypeError, called); }
try { s.replaceAll(re, "x"); console.log("no throw"); } catch (e: any) { console.log(e instanceof TypeError, called); }
console.log(s.replace(re, "x"), called);
const g: any = /a/g;
g[Symbol.matchAll] = function () { called++; return "customG"; };
console.log(s.matchAll(g), called);
const o: any = { [Symbol.search]: (str: string) => str.length * 10 };
console.log(s.search(o));
const m: any = { [Symbol.match]: (str: string) => ["M", str] };
console.log(s.match(m, 1, 2));
const sp: any = { [Symbol.split]: (str: string, lim: any) => [str, lim] };
console.log(s.split(sp, 3));
console.log(s.split("b"), s.match(/b/g), s.search("b"), s.replace("a", "z"), s.replaceAll("a", "z"));
const bad: any = { toString() { throw new Error("ts"); } };
try { s.match(bad); } catch (e: any) { console.log("match", e.message); }
try { s.search(bad); } catch (e: any) { console.log("search", e.message); }
try { s.matchAll(bad); } catch (e: any) { console.log("matchAll", e.message); }
