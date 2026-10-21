// Tauri v2 rejects `Result<T, String>` commands with the plain string, not an
// Error object, so `err instanceof Error` alone loses the backend message.
export const invokeErrorMessage = (err: unknown, fallback: string): string => {
  if (typeof err === "string") return err.trim() || fallback;
  if (err instanceof Error) return err.message || fallback;
  if (err === null || err === undefined) return fallback;
  return String(err) || fallback;
};
