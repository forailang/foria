/**
 * log.* ops → browser console methods.
 *
 * log.debug(message, context?) → console.debug
 * log.info(message, context?)  → console.log
 * log.warn(message, context?)  → console.warn
 * log.error(message, context?) → console.error
 * log.trace(message, context?) → console.trace
 */
export function handleLogOp(op: string, args: unknown[]): unknown {
  const message = args[0] ?? "";
  const context = args[1];

  switch (op) {
    case "log.debug":
      context !== undefined ? console.debug(message, context) : console.debug(message);
      break;
    case "log.info":
      context !== undefined ? console.log(message, context) : console.log(message);
      break;
    case "log.warn":
      context !== undefined ? console.warn(message, context) : console.warn(message);
      break;
    case "log.error":
      context !== undefined ? console.error(message, context) : console.error(message);
      break;
    case "log.trace":
      context !== undefined ? console.trace(message, context) : console.trace(message);
      break;
    default:
      throw new Error(`unknown log op: ${op}`);
  }

  return true;
}
