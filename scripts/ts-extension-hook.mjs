export async function resolve(specifier, context, nextResolve) {
  const parent = context.parentURL ?? "";
  const fromSource = parent.includes("/frontend/src/") || parent.includes("/scripts/");
  const file = specifier.split("/").pop() ?? specifier;
  if (fromSource && (specifier.startsWith("./") || specifier.startsWith("../")) && !file.includes(".")) {
    return nextResolve(`${specifier}.ts`, context);
  }
  return nextResolve(specifier, context);
}
