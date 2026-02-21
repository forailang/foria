/**
 * HTTP client ops using the browser's fetch() API.
 *
 * All ops return the same shape as the native host:
 *   { status: number, headers: { lowercase: value }, body: string }
 *
 * Supported ops:
 *   http.get(url, options?)
 *   http.post(url, body?, options?)
 *   http.put(url, body?, options?)
 *   http.patch(url, body?, options?)
 *   http.delete(url, options?)
 *   http.request(method, url, options?)
 *
 * Options object (optional):
 *   { headers?: Record<string, string>, timeout_ms?: number }
 */

interface HttpOptions {
  headers?: Record<string, string>;
  timeout_ms?: number;
}

interface HttpResponse {
  status: number;
  headers: Record<string, string>;
  body: string;
}

async function doFetch(
  method: string,
  url: string,
  body: string | undefined,
  options: HttpOptions,
): Promise<HttpResponse> {
  const timeoutMs = options.timeout_ms ?? 30000;

  const init: RequestInit = {
    method: method.toUpperCase(),
    headers: options.headers,
  };

  if (body !== undefined && body !== "") {
    init.body = body;
  }

  // AbortController for timeout
  const controller = new AbortController();
  init.signal = controller.signal;
  const timer = setTimeout(() => controller.abort(), timeoutMs);

  try {
    const resp = await fetch(url, init);

    // Collect headers (lowercase keys)
    const headers: Record<string, string> = {};
    resp.headers.forEach((value, key) => {
      headers[key.toLowerCase()] = value;
    });

    const respBody = await resp.text();

    return {
      status: resp.status,
      headers,
      body: respBody,
    };
  } finally {
    clearTimeout(timer);
  }
}

function parseOptions(arg: unknown): HttpOptions {
  if (arg && typeof arg === "object" && !Array.isArray(arg)) {
    return arg as HttpOptions;
  }
  return {};
}

export function handleHttpOp(
  op: string,
  args: unknown[],
): Promise<HttpResponse> {
  switch (op) {
    case "http.get": {
      const url = String(args[0]);
      const options = parseOptions(args[1]);
      return doFetch("GET", url, undefined, options);
    }

    case "http.post": {
      const url = String(args[0]);
      const body = args[1] !== undefined ? String(args[1]) : undefined;
      const options = parseOptions(args[2]);
      return doFetch("POST", url, body, options);
    }

    case "http.put": {
      const url = String(args[0]);
      const body = args[1] !== undefined ? String(args[1]) : undefined;
      const options = parseOptions(args[2]);
      return doFetch("PUT", url, body, options);
    }

    case "http.patch": {
      const url = String(args[0]);
      const body = args[1] !== undefined ? String(args[1]) : undefined;
      const options = parseOptions(args[2]);
      return doFetch("PATCH", url, body, options);
    }

    case "http.delete": {
      const url = String(args[0]);
      const options = parseOptions(args[1]);
      return doFetch("DELETE", url, undefined, options);
    }

    case "http.request": {
      const method = String(args[0]);
      const url = String(args[1]);
      const options = parseOptions(args[2]);
      const body = options && typeof (options as Record<string, unknown>).body === "string"
        ? (options as Record<string, unknown>).body as string
        : undefined;
      return doFetch(method, url, body, options);
    }

    default:
      throw new Error(`unknown http op: ${op}`);
  }
}
