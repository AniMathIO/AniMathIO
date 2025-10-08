export function isValidComponent(component: unknown): boolean {
  return (
    typeof component === "function" ||
    (typeof component === "object" && component !== null)
  );
}
