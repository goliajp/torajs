// toLocale{Lower,Upper}Case locale validation (BCP47) + array locales
// invalid string locale -> RangeError
try { "abc".toLocaleLowerCase("not a locale"); } catch (e) { console.log((e as Error).name, (e as Error).message); }
try { "abc".toLocaleUpperCase(""); } catch (e) { console.log((e as Error).name, (e as Error).message); }
try { "abc".toLocaleLowerCase("en--US"); } catch (e) { console.log((e as Error).name, (e as Error).message); }
try { "abc".toLocaleLowerCase("x-priv"); } catch (e) { console.log((e as Error).name, (e as Error).message); }
// valid complex tags do not throw
console.log("I".toLocaleLowerCase("de-DE-u-co-phonebk"));
console.log("I".toLocaleLowerCase("zh-Hans-CN"));
console.log("I".toLocaleLowerCase("en-US-x-priv"));
console.log("I".toLocaleLowerCase("ca-ES-valencia"));
console.log("I".toLocaleLowerCase("es-419"));
try { "I".toLocaleLowerCase("root"); } catch (e) { console.log((e as Error).name, (e as Error).message); }
// array locales: all validated, first selects tailoring
const locs1 = ["en-US", "this is not a valid locale"];
try { "".toLocaleLowerCase(locs1); } catch (e) { console.log((e as Error).name, (e as Error).message); }
console.log("I".toLocaleLowerCase(["tr"]));
console.log("I".toLocaleLowerCase(["en-US", "tr"]));
console.log("i".toLocaleUpperCase(["az", "en-US"]));
const empty: string[] = [];
console.log("I".toLocaleLowerCase(empty));
// array via any-tier receiver
const anyRecv: any = "I";
console.log(anyRecv.toLocaleLowerCase(["tr", "lt"]));
try { anyRecv.toLocaleLowerCase(["en-US", "bogus tag here"]); } catch (e) { console.log((e as Error).name, (e as Error).message); }
try { anyRecv.toLocaleUpperCase("de_DE"); } catch (e) { console.log((e as Error).name, (e as Error).message); }
// Substr receiver with validation
const big2 = "xxIyy";
try { big2.slice(2, 3).toLocaleLowerCase("*"); } catch (e) { console.log((e as Error).name, (e as Error).message); }
console.log(big2.slice(2, 3).toLocaleLowerCase(["TR-tr"]));
