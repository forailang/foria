/**
 * term.* ops — terminal I/O in the browser.
 *
 * term.print(value)   → onPrint callback (default: console.log)
 * term.prompt(message) → onPrompt callback (default: window.prompt)
 * term.clear()        → onPrint("") or console.clear
 * term.size()         → fixed size { cols: 80, rows: 24 }
 *
 * Other term ops (cursor, move_to, color, read_key) are stubbed.
 */

export interface TermCallbacks {
  onPrint?: (text: string) => void;
  onPrompt?: (message: string) => string | Promise<string>;
}

// Options are injected by the host dispatcher
const _opts: TermCallbacks = {};

export function handleTermOp(
  op: string,
  args: unknown[],
): unknown | Promise<unknown> {
  // Read injected options
  const opts: TermCallbacks =
    (handleTermOp as unknown as { _opts?: TermCallbacks })._opts ?? _opts;

  switch (op) {
    case "term.print": {
      const text = String(args[0] ?? "");
      if (opts.onPrint) {
        opts.onPrint(text);
      } else {
        console.log(text);
      }
      return true;
    }

    case "term.prompt": {
      const message = String(args[0] ?? "");
      if (opts.onPrompt) {
        return opts.onPrompt(message);
      }
      // Default: use window.prompt (sync in browsers)
      if (typeof window !== "undefined" && window.prompt) {
        return window.prompt(message) ?? "";
      }
      return "";
    }

    case "term.clear": {
      if (typeof console !== "undefined" && console.clear) {
        console.clear();
      }
      return true;
    }

    case "term.size":
      return { cols: 80, rows: 24 };

    case "term.cursor":
      return { col: 0, row: 0 };

    case "term.move_to":
    case "term.color":
    case "term.read_key":
      throw new Error(`${op} is not available in the browser`);

    default:
      throw new Error(`unknown term op: ${op}`);
  }
}
