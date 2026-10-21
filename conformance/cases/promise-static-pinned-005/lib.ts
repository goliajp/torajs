export function install(): void {
  (Promise as any).resolve = function (v: any) {
    console.log("lib-patched", v)
    return new Promise((res: any) => res("lib:" + v))
  }
}
