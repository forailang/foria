/**
 * time.* ops in the browser.
 *
 * time.sleep(seconds) → setTimeout-based delay, returns true
 */
export function handleTimeOp(
  op: string,
  args: unknown[],
): unknown | Promise<unknown> {
  switch (op) {
    case "time.sleep": {
      const seconds = Number(args[0]);
      if (isNaN(seconds) || seconds < 0) {
        throw new Error("time.sleep: argument must be a non-negative number");
      }
      const ms = Math.floor(seconds * 1000);
      return new Promise<boolean>((resolve) => setTimeout(() => resolve(true), ms));
    }

    default:
      throw new Error(`unknown time op: ${op}`);
  }
}
