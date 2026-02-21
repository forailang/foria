/**
 * Stub handlers for ops unavailable in the browser.
 * Returns clear error messages explaining why the op can't work.
 */

const UNAVAILABLE: Record<string, string> = {
  "file.": "file I/O is not available in the browser",
  "exec.": "process execution is not available in the browser",
  "http.server.": "HTTP server is not available in the browser (use a Service Worker instead)",
  "db.": "SQLite database is not available in the browser (sql.js support coming soon)",
  "env.": "environment variables are not available in the browser",
};

export function handleStubOp(op: string, _args: unknown[]): never {
  for (const [prefix, message] of Object.entries(UNAVAILABLE)) {
    if (op.startsWith(prefix)) {
      throw new Error(`${op}: ${message}`);
    }
  }
  throw new Error(`${op}: not available in the browser`);
}
