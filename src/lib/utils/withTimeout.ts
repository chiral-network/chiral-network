/// Race a promise against a timeout. If the promise settles first (resolve or
/// reject), the timer is cleared and that outcome is returned. If the timer
/// fires first, the returned promise rejects with a descriptive message.
///
/// Kept as a standalone utility rather than inlined per call-site so we don't
/// leak the timer handle on every retry, and so the behavior is unit-testable.
export async function withTimeout<T>(
  promise: Promise<T>,
  timeoutMs: number,
  operation: string,
): Promise<T> {
  let timer: ReturnType<typeof setTimeout> | null = null;
  const timeoutPromise = new Promise<never>((_, reject) => {
    timer = setTimeout(
      () => reject(new Error(`${operation} timed out after ${timeoutMs}ms`)),
      timeoutMs,
    );
  });
  try {
    return await Promise.race([promise, timeoutPromise]);
  } finally {
    if (timer) clearTimeout(timer);
  }
}
