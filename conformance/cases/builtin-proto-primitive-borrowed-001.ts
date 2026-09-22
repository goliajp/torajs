// §21.1.3 / §20.3.3 / §22.1.3 — the Number, Boolean and String
// prototypes are themselves objects with the primitive's internal
// slot (+0, false, ""). A borrowed toString / valueOf called on the
// prototype reads that slot instead of failing the brand check.
console.log((Number.prototype.toString as any).call(Number.prototype));
console.log((Number.prototype.valueOf as any).call(Number.prototype));
console.log((Boolean.prototype.toString as any).call(Boolean.prototype));
console.log((Boolean.prototype.valueOf as any).call(Boolean.prototype));
console.log(JSON.stringify((String.prototype.toString as any).call(String.prototype)));
console.log(JSON.stringify((String.prototype.valueOf as any).call(String.prototype)));
console.log((Number.prototype.toFixed as any).call(Number.prototype, 2));
console.log(String(Number.prototype), String(Boolean.prototype), JSON.stringify(String(String.prototype)));
try { (Number.prototype.toString as any).call(Boolean.prototype); } catch (e) { console.log((e as Error).constructor === TypeError); }
try { (Boolean.prototype.valueOf as any).call(Number.prototype); } catch (e) { console.log((e as Error).constructor === TypeError); }
console.log(Object.prototype.toString.call(Number.prototype), Object.prototype.toString.call(Boolean.prototype));
