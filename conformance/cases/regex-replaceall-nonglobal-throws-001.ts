// §22.1.5 — String.prototype.replaceAll throws a TypeError when the
// RegExp searchValue is not global (both string- and callback-replacement).
try { "aXbXc".replaceAll(/X/, "-"); } catch (e: any) { console.log("str:", e.name); }
try { "aXb".replaceAll(/X/, (m) => m.toLowerCase()); } catch (e: any) { console.log("cb:", e.name); }
console.log("global-str:", "aXbXc".replaceAll(/X/g, "-"));
console.log("global-cb:", "aXbX".replaceAll(/X/g, (m) => "[" + m + "]"));
console.log("replace-ok:", "aXbX".replace(/X/, "-"));
console.log("string-pat:", "a.b.c".replaceAll(".", "-"));
