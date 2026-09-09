// The array mutators refuse in spec order, and bun words each refusal
// by the step that failed: a non-writable `length` (a freeze or a
// defineProperty lock) refuses every `length =` write, the same value
// included, as "Array length is not writable"; a step that deletes an
// element (pop / shift / a shrinking splice / a shrinking length on a
// sealed or frozen array) as "Unable to delete property."; a step
// that stores (push / unshift / sort / the first move of a
// multi-element shift on a frozen array) as the readonly assignment.
// And an empty sealed array's pop / shift only rewrites an unchanged
// writable length, which is legal — before this it threw.
function t(tag: string, f: () => unknown) { try { f(); console.log(tag, "ok"); } catch (e) { console.log(tag, (e as Error).constructor.name, (e as Error).message); } }
t("frozen length=", () => { const xs: any = [1]; Object.freeze(xs); xs.length = 0; });
t("sealed length=", () => { const xs: any = [1]; Object.seal(xs); xs.length = 0; });
t("sealed length grow", () => { const xs: any = [1]; Object.seal(xs); xs.length = 5; });
t("RO length=", () => { const xs: any = [1]; Object.defineProperty(xs, "length", { writable: false }); xs.length = 0; });
t("RO length same", () => { const xs: any = [1]; Object.defineProperty(xs, "length", { writable: false }); xs.length = 1; });
t("frozen pop", () => { const xs: any = [1]; Object.freeze(xs); xs.pop(); });
t("sealed pop", () => { const xs: any = [1]; Object.seal(xs); xs.pop(); });
t("frozen shift", () => { const xs: any = [1]; Object.freeze(xs); xs.shift(); });
t("frozen push", () => { const xs: any = [1]; Object.freeze(xs); xs.push(1); });
t("RO length push", () => { const xs: any = [1]; Object.defineProperty(xs, "length", { writable: false }); xs.push(1); });
t("RO length pop", () => { const xs: any = [1]; Object.defineProperty(xs, "length", { writable: false }); xs.pop(); });
t("frozen elem=", () => { const xs: any = [1]; Object.freeze(xs); xs[0] = 2; });
t("frozen typed length=", () => { const xs: number[] = [1]; Object.freeze(xs); xs.length = 0; });
t("Reflect.set length frozen", () => { const xs: any = [1]; Object.freeze(xs); console.log(Reflect.set(xs, "length", 0)); });
t("preventExt push", () => { const xs: any = [1]; Object.preventExtensions(xs); xs.push(2); });
t("sealed push", () => { const xs: any = [1]; Object.seal(xs); xs.push(2); });
t("sealed unshift", () => { const xs: any = [1]; Object.seal(xs); xs.unshift(2); });
t("frozen unshift", () => { const xs: any = [1]; Object.freeze(xs); xs.unshift(2); });
t("frozen shift 2", () => { const xs: any = [1, 2]; Object.freeze(xs); xs.shift(); });
t("frozen pop 2", () => { const xs: any = [1, 2]; Object.freeze(xs); xs.pop(); });
t("sealed shift 2", () => { const xs: any = [1, 2]; Object.seal(xs); xs.shift(); });
t("frozen splice", () => { const xs: any = [1, 2]; Object.freeze(xs); xs.splice(0, 1); });
t("sealed splice", () => { const xs: any = [1, 2]; Object.seal(xs); xs.splice(0, 1); });
t("frozen sort", () => { const xs: any = [2, 1]; Object.freeze(xs); xs.sort(); });
t("frozen reverse", () => { const xs: any = [2, 1]; Object.freeze(xs); xs.reverse(); });
t("frozen fill", () => { const xs: any = [2, 1]; Object.freeze(xs); xs.fill(0); });
t("RO length shift", () => { const xs: any = [1, 2]; Object.defineProperty(xs, "length", { writable: false }); xs.shift(); });
t("RO length length=big", () => { const xs: any = [1]; Object.defineProperty(xs, "length", { writable: false }); xs.length = 5; });
t("frozen length=same", () => { const xs: any = [1]; Object.freeze(xs); xs.length = 1; });
t("typed frozen length=", () => { const xs: number[] = [1, 2]; Object.freeze(xs); xs.length = 1; });
t("typed sealed length=", () => { const xs: number[] = [1, 2]; Object.seal(xs); xs.length = 1; });
t("frozen [] shift", () => { const xs: any = []; Object.freeze(xs); xs.shift(); });
t("frozen [] pop", () => { const xs: any = []; Object.freeze(xs); xs.pop(); });
t("sealed [] pop", () => { const xs: any = []; Object.seal(xs); xs.pop(); });
t("sealed [] shift", () => { const xs: any = []; Object.seal(xs); xs.shift(); });
t("RO [] pop", () => { const xs: any = []; Object.defineProperty(xs, "length", { writable: false }); xs.pop(); });
t("RO [] shift", () => { const xs: any = []; Object.defineProperty(xs, "length", { writable: false }); xs.shift(); });
t("RO [1] pop", () => { const xs: any = [1]; Object.defineProperty(xs, "length", { writable: false }); xs.pop(); });
t("sealed length= grow typed", () => { const xs: number[] = [1]; Object.seal(xs); xs.length = 3; console.log(xs.length); });
t("frozen length= grow", () => { const xs: any = [1]; Object.freeze(xs); xs.length = 3; });
