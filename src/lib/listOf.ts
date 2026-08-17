/**
 * The list out of a response, whichever way it was wrapped.
 *
 * Most list endpoints answer `{ items: [...] }` and a few answer a bare array.
 * Callers stored whatever arrived, so a page that received the envelope was
 * holding an object where it expected a list: `.find` and `for…of` threw, or
 * the page simply rendered nothing and looked like an empty register.
 *
 * That is how the duty rules stopped reaching the calculator, and how the
 * revenue module showed no shifts. Reading through this means a page cannot be
 * broken again by which shape an endpoint happens to use.
 */
export function listOf<T = unknown>(data: unknown): T[] {
  if (Array.isArray(data)) return data as T[];
  const items = (data as { items?: unknown } | null | undefined)?.items;
  return Array.isArray(items) ? (items as T[]) : [];
}
