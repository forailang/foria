/**
 * WebSocket ops using the browser WebSocket API.
 *
 * ws.connect(url)       → open a WebSocket, return handle string
 * ws.send(handle, data) → send text message, return true
 * ws.recv(handle)       → wait for next message, return { type, data }
 * ws.close(handle)      → close the connection, return true
 */

interface WsHandle {
  ws: WebSocket;
  messageQueue: MessageEvent[];
  waiters: ((msg: MessageEvent) => void)[];
  closed: boolean;
}

const handles = new Map<string, WsHandle>();
let nextId = 0;

function nextHandle(): string {
  return `ws_${nextId++}`;
}

function getHandle(id: string): WsHandle {
  const h = handles.get(id);
  if (!h) throw new Error(`ws: invalid handle ${id}`);
  return h;
}

export function handleWsOp(
  op: string,
  args: unknown[],
): unknown | Promise<unknown> {
  switch (op) {
    case "ws.connect": {
      const url = String(args[0]);
      return new Promise<string>((resolve, reject) => {
        const ws = new WebSocket(url);
        const id = nextHandle();
        const handle: WsHandle = {
          ws,
          messageQueue: [],
          waiters: [],
          closed: false,
        };

        ws.onopen = () => {
          handles.set(id, handle);
          resolve(id);
        };

        ws.onerror = () => {
          reject(new Error(`ws.connect: failed to connect to ${url}`));
        };

        ws.onmessage = (event: MessageEvent) => {
          if (handle.waiters.length > 0) {
            const waiter = handle.waiters.shift()!;
            waiter(event);
          } else {
            handle.messageQueue.push(event);
          }
        };

        ws.onclose = () => {
          handle.closed = true;
          // Wake any waiters with a close event
          for (const waiter of handle.waiters) {
            waiter(new MessageEvent("close", { data: "" }));
          }
          handle.waiters.length = 0;
        };
      });
    }

    case "ws.send": {
      const id = String(args[0]);
      const data = String(args[1]);
      const h = getHandle(id);
      if (h.closed) throw new Error("ws.send: connection is closed");
      h.ws.send(data);
      return true;
    }

    case "ws.recv": {
      const id = String(args[0]);
      const h = getHandle(id);

      // If there's a queued message, return it immediately
      if (h.messageQueue.length > 0) {
        const event = h.messageQueue.shift()!;
        return formatMessage(event);
      }

      if (h.closed) {
        return { type: "close", data: "" };
      }

      // Wait for next message
      return new Promise<unknown>((resolve) => {
        h.waiters.push((event: MessageEvent) => {
          resolve(formatMessage(event));
        });
      });
    }

    case "ws.close": {
      const id = String(args[0]);
      const h = getHandle(id);
      h.ws.close();
      h.closed = true;
      handles.delete(id);
      return true;
    }

    default:
      throw new Error(`unknown ws op: ${op}`);
  }
}

function formatMessage(event: MessageEvent): unknown {
  if (event.type === "close") {
    return { type: "close", data: "" };
  }
  // Text messages
  if (typeof event.data === "string") {
    return { type: "text", data: event.data };
  }
  // Binary (ArrayBuffer or Blob)
  return { type: "binary", data: String(event.data) };
}
