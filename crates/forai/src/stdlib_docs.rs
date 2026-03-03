use crate::doc::{OpArgDoc, OpReturnDoc, StdlibNamespaceDoc, StdlibOpDoc};

// Builder helpers for compact declaration

fn arg(position: usize, name: &str, type_name: &str, description: &str) -> OpArgDoc {
    OpArgDoc {
        position,
        name: name.to_string(),
        type_name: type_name.to_string(),
        description: description.to_string(),
    }
}

fn ret(type_name: &str, description: &str) -> OpReturnDoc {
    OpReturnDoc {
        type_name: type_name.to_string(),
        description: description.to_string(),
    }
}

fn op(
    name: &str,
    ns: &str,
    summary: &str,
    args: Vec<OpArgDoc>,
    returns: OpReturnDoc,
) -> StdlibOpDoc {
    StdlibOpDoc {
        name: name.to_string(),
        full_name: format!("{ns}.{name}"),
        summary: summary.to_string(),
        args,
        returns,
        errors: None,
    }
}

fn op_err(
    name: &str,
    ns: &str,
    summary: &str,
    args: Vec<OpArgDoc>,
    returns: OpReturnDoc,
    errors: &str,
) -> StdlibOpDoc {
    StdlibOpDoc {
        name: name.to_string(),
        full_name: format!("{ns}.{name}"),
        summary: summary.to_string(),
        args,
        returns,
        errors: Some(errors.to_string()),
    }
}

fn ns(namespace: &str, summary: &str, ops: Vec<StdlibOpDoc>) -> StdlibNamespaceDoc {
    StdlibNamespaceDoc {
        namespace: namespace.to_string(),
        summary: summary.to_string(),
        ops,
    }
}

pub fn all_stdlib_docs() -> Vec<StdlibNamespaceDoc> {
    vec![
        str_docs(),
        math_docs(),
        obj_docs(),
        list_docs(),
        type_docs(),
        to_docs(),
        json_docs(),
        codec_docs(),
        http_docs(),
        http_server_docs(),
        http_respond_docs(),
        ws_docs(),
        headers_docs(),
        auth_docs(),
        db_docs(),
        date_docs(),
        stamp_docs(),
        trange_docs(),
        time_docs(),
        fmt_docs(),
        term_docs(),
        file_docs(),
        env_docs(),
        exec_docs(),
        regex_docs(),
        random_docs(),
        hash_docs(),
        base64_docs(),
        crypto_docs(),
        log_docs(),
        error_docs(),
        cookie_docs(),
        url_docs(),
        route_docs(),
        html_docs(),
        tmpl_docs(),
        ffi_docs(),
    ]
}

fn str_docs() -> StdlibNamespaceDoc {
    ns(
        "str",
        "String manipulation operations",
        vec![
            op(
                "len",
                "str",
                "Returns the length of a string in characters",
                vec![arg(0, "s", "text", "Input string")],
                ret("long", "Character count"),
            ),
            op(
                "upper",
                "str",
                "Converts string to uppercase",
                vec![arg(0, "s", "text", "Input string")],
                ret("text", "Uppercased string"),
            ),
            op(
                "lower",
                "str",
                "Converts string to lowercase",
                vec![arg(0, "s", "text", "Input string")],
                ret("text", "Lowercased string"),
            ),
            op(
                "trim",
                "str",
                "Removes whitespace from both ends",
                vec![arg(0, "s", "text", "Input string")],
                ret("text", "Trimmed string"),
            ),
            op(
                "trim_start",
                "str",
                "Removes leading whitespace",
                vec![arg(0, "s", "text", "Input string")],
                ret("text", "String with leading whitespace removed"),
            ),
            op(
                "trim_end",
                "str",
                "Removes trailing whitespace",
                vec![arg(0, "s", "text", "Input string")],
                ret("text", "String with trailing whitespace removed"),
            ),
            op(
                "split",
                "str",
                "Splits a string by a delimiter",
                vec![
                    arg(0, "s", "text", "Input string"),
                    arg(1, "delim", "text", "Delimiter to split on"),
                ],
                ret("list", "List of substrings"),
            ),
            op(
                "join",
                "str",
                "Joins a list of strings with a separator",
                vec![
                    arg(0, "parts", "list", "List of strings"),
                    arg(1, "sep", "text", "Separator"),
                ],
                ret("text", "Joined string"),
            ),
            op(
                "replace",
                "str",
                "Replaces all occurrences of a pattern",
                vec![
                    arg(0, "s", "text", "Input string"),
                    arg(1, "from", "text", "Pattern to find"),
                    arg(2, "to", "text", "Replacement"),
                ],
                ret("text", "String with replacements applied"),
            ),
            op(
                "contains",
                "str",
                "Checks if string contains a substring",
                vec![
                    arg(0, "s", "text", "Input string"),
                    arg(1, "sub", "text", "Substring to search for"),
                ],
                ret("bool", "True if substring is found"),
            ),
            op(
                "starts_with",
                "str",
                "Checks if string starts with a prefix",
                vec![
                    arg(0, "s", "text", "Input string"),
                    arg(1, "prefix", "text", "Prefix to check"),
                ],
                ret("bool", "True if string starts with prefix"),
            ),
            op(
                "ends_with",
                "str",
                "Checks if string ends with a suffix",
                vec![
                    arg(0, "s", "text", "Input string"),
                    arg(1, "suffix", "text", "Suffix to check"),
                ],
                ret("bool", "True if string ends with suffix"),
            ),
            op(
                "slice",
                "str",
                "Extracts a substring by start and end indices",
                vec![
                    arg(0, "s", "text", "Input string"),
                    arg(1, "start", "long", "Start index (0-based)"),
                    arg(2, "end", "long", "End index (exclusive)"),
                ],
                ret("text", "Extracted substring"),
            ),
            op(
                "index_of",
                "str",
                "Finds the first index of a substring",
                vec![
                    arg(0, "s", "text", "Input string"),
                    arg(1, "sub", "text", "Substring to find"),
                ],
                ret("long", "Index of first occurrence, or -1 if not found"),
            ),
            op(
                "repeat",
                "str",
                "Repeats a string N times",
                vec![
                    arg(0, "s", "text", "Input string"),
                    arg(1, "n", "long", "Number of repetitions"),
                ],
                ret("text", "Repeated string"),
            ),
        ],
    )
}

fn math_docs() -> StdlibNamespaceDoc {
    ns(
        "math",
        "Rounding operations (arithmetic uses infix operators: + - * / % **)",
        vec![
            op(
                "floor",
                "math",
                "Rounds down to nearest integer",
                vec![arg(0, "n", "real", "Input number")],
                ret("long", "Floor value"),
            ),
            op(
                "round",
                "math",
                "Rounds to N decimal places",
                vec![
                    arg(0, "n", "real", "Input number"),
                    arg(1, "places", "long", "Number of decimal places"),
                ],
                ret("real", "Rounded value"),
            ),
        ],
    )
}

fn obj_docs() -> StdlibNamespaceDoc {
    ns(
        "obj",
        "Immutable dictionary operations",
        vec![
            op(
                "new",
                "obj",
                "Creates an empty dictionary",
                vec![],
                ret("dict", "Empty dictionary"),
            ),
            op(
                "set",
                "obj",
                "Returns a new dictionary with a key set",
                vec![
                    arg(0, "d", "dict", "Input dictionary"),
                    arg(1, "key", "text", "Key to set"),
                    arg(2, "value", "any", "Value to set"),
                ],
                ret("dict", "New dictionary with the key set"),
            ),
            op(
                "get",
                "obj",
                "Gets a value by key",
                vec![
                    arg(0, "d", "dict", "Input dictionary"),
                    arg(1, "key", "text", "Key to look up"),
                ],
                ret("any", "Value at key, or void if missing"),
            ),
            op(
                "has",
                "obj",
                "Checks if a key exists",
                vec![
                    arg(0, "d", "dict", "Input dictionary"),
                    arg(1, "key", "text", "Key to check"),
                ],
                ret("bool", "True if key exists"),
            ),
            op(
                "delete",
                "obj",
                "Returns a new dictionary with a key removed",
                vec![
                    arg(0, "d", "dict", "Input dictionary"),
                    arg(1, "key", "text", "Key to remove"),
                ],
                ret("dict", "New dictionary without the key"),
            ),
            op(
                "keys",
                "obj",
                "Returns all keys as a list",
                vec![arg(0, "d", "dict", "Input dictionary")],
                ret("list", "List of key strings"),
            ),
            op(
                "merge",
                "obj",
                "Merges two dictionaries (right wins on conflict)",
                vec![
                    arg(0, "a", "dict", "Base dictionary"),
                    arg(1, "b", "dict", "Override dictionary"),
                ],
                ret("dict", "Merged dictionary"),
            ),
        ],
    )
}

fn list_docs() -> StdlibNamespaceDoc {
    ns(
        "list",
        "Immutable list operations",
        vec![
            op(
                "new",
                "list",
                "Creates an empty list",
                vec![],
                ret("list", "Empty list"),
            ),
            op(
                "range",
                "list",
                "Creates a list of integers from start to end (exclusive)",
                vec![
                    arg(0, "start", "long", "Start value (inclusive)"),
                    arg(1, "end", "long", "End value (exclusive)"),
                ],
                ret("list", "List of integers"),
            ),
            op(
                "append",
                "list",
                "Returns a new list with an item appended",
                vec![
                    arg(0, "l", "list", "Input list"),
                    arg(1, "item", "any", "Item to append"),
                ],
                ret("list", "New list with item at end"),
            ),
            op(
                "len",
                "list",
                "Returns the number of items",
                vec![arg(0, "l", "list", "Input list")],
                ret("long", "Number of items"),
            ),
            op(
                "contains",
                "list",
                "Checks if an item is in the list",
                vec![
                    arg(0, "l", "list", "Input list"),
                    arg(1, "item", "any", "Item to search for"),
                ],
                ret("bool", "True if item is found"),
            ),
            op(
                "slice",
                "list",
                "Extracts a sublist by start and end indices",
                vec![
                    arg(0, "l", "list", "Input list"),
                    arg(1, "start", "long", "Start index (0-based)"),
                    arg(2, "end", "long", "End index (exclusive)"),
                ],
                ret("list", "Extracted sublist"),
            ),
            op(
                "indices",
                "list",
                "Returns a list of indices for the input list",
                vec![arg(0, "l", "list", "Input list")],
                ret("list", "List of integer indices [0, 1, ..., len-1]"),
            ),
        ],
    )
}

fn type_docs() -> StdlibNamespaceDoc {
    ns(
        "type",
        "Type introspection",
        vec![op(
            "of",
            "type",
            "Returns the type name of a value",
            vec![arg(0, "value", "any", "Value to inspect")],
            ret(
                "text",
                "Type name: text, bool, long, real, list, dict, or void",
            ),
        )],
    )
}

fn to_docs() -> StdlibNamespaceDoc {
    ns(
        "to",
        "Type conversion between scalars",
        vec![
            op(
                "text",
                "to",
                "Converts a value to text",
                vec![arg(0, "value", "any", "Value to convert")],
                ret("text", "String representation"),
            ),
            op_err(
                "long",
                "to",
                "Converts a value to a long integer",
                vec![arg(0, "value", "any", "Value to convert")],
                ret("long", "Integer value"),
                "Fails if value cannot be parsed as integer",
            ),
            op_err(
                "real",
                "to",
                "Converts a value to a real number",
                vec![arg(0, "value", "any", "Value to convert")],
                ret("real", "Float value"),
                "Fails if value cannot be parsed as number",
            ),
            op(
                "bool",
                "to",
                "Converts a value to boolean",
                vec![arg(0, "value", "any", "Value to convert")],
                ret("bool", "Boolean value"),
            ),
        ],
    )
}

fn json_docs() -> StdlibNamespaceDoc {
    ns(
        "json",
        "JSON codec operations",
        vec![
            op_err(
                "decode",
                "json",
                "Parses a JSON string into a value",
                vec![arg(0, "s", "text", "JSON string")],
                ret("any", "Parsed value"),
                "Fails if input is not valid JSON",
            ),
            op(
                "encode",
                "json",
                "Serializes a value to compact JSON",
                vec![arg(0, "value", "any", "Value to serialize")],
                ret("text", "Compact JSON string"),
            ),
            op(
                "encode_pretty",
                "json",
                "Serializes a value to pretty-printed JSON",
                vec![arg(0, "value", "any", "Value to serialize")],
                ret("text", "Pretty-printed JSON string"),
            ),
        ],
    )
}

fn codec_docs() -> StdlibNamespaceDoc {
    ns(
        "codec",
        "Generic codec dispatch by format name",
        vec![
            op_err(
                "decode",
                "codec",
                "Decodes a string using the named format",
                vec![
                    arg(0, "format", "text", "Codec format name"),
                    arg(1, "s", "text", "Encoded string"),
                ],
                ret("any", "Decoded value"),
                "Fails if format is unknown or input is invalid",
            ),
            op(
                "encode",
                "codec",
                "Encodes a value using the named format",
                vec![
                    arg(0, "format", "text", "Codec format name"),
                    arg(1, "value", "any", "Value to encode"),
                ],
                ret("text", "Encoded string"),
            ),
            op(
                "encode_pretty",
                "codec",
                "Pretty-encodes a value using the named format",
                vec![
                    arg(0, "format", "text", "Codec format name"),
                    arg(1, "value", "any", "Value to encode"),
                ],
                ret("text", "Pretty-encoded string"),
            ),
        ],
    )
}

fn http_docs() -> StdlibNamespaceDoc {
    ns(
        "http",
        "HTTP client and response helpers",
        vec![
            op(
                "extract_path",
                "http",
                "Extracts the path from an HTTP request object",
                vec![arg(0, "request", "dict", "HTTP request object")],
                ret("text", "Request path"),
            ),
            op(
                "extract_params",
                "http",
                "Extracts parameters from an HTTP request body",
                vec![arg(0, "request", "dict", "HTTP request object")],
                ret("dict", "Extracted parameters"),
            ),
            op(
                "response",
                "http",
                "Creates an HTTP response object",
                vec![
                    arg(0, "status", "long", "HTTP status code"),
                    arg(1, "body", "any", "Response body"),
                ],
                ret("dict", "HTTP response with status and body"),
            ),
            op(
                "error_response",
                "http",
                "Creates an HTTP error response object",
                vec![
                    arg(0, "status", "long", "HTTP status code"),
                    arg(1, "message", "text", "Error message"),
                ],
                ret("dict", "HTTP error response"),
            ),
            op_err(
                "get",
                "http",
                "Performs an HTTP GET request",
                vec![
                    arg(0, "url", "text", "Request URL"),
                    arg(
                        1,
                        "options",
                        "dict",
                        "Request options (headers, timeout_ms)",
                    ),
                ],
                ret("dict", "Response with status, headers, body"),
                "Fails on network or timeout errors",
            ),
            op_err(
                "post",
                "http",
                "Performs an HTTP POST request",
                vec![
                    arg(0, "url", "text", "Request URL"),
                    arg(1, "body", "any", "Request body"),
                    arg(2, "options", "dict", "Request options"),
                ],
                ret("dict", "Response with status, headers, body"),
                "Fails on network or timeout errors",
            ),
            op_err(
                "put",
                "http",
                "Performs an HTTP PUT request",
                vec![
                    arg(0, "url", "text", "Request URL"),
                    arg(1, "body", "any", "Request body"),
                    arg(2, "options", "dict", "Request options"),
                ],
                ret("dict", "Response with status, headers, body"),
                "Fails on network or timeout errors",
            ),
            op_err(
                "delete",
                "http",
                "Performs an HTTP DELETE request",
                vec![
                    arg(0, "url", "text", "Request URL"),
                    arg(1, "options", "dict", "Request options"),
                ],
                ret("dict", "Response with status, headers, body"),
                "Fails on network or timeout errors",
            ),
            op_err(
                "patch",
                "http",
                "Performs an HTTP PATCH request",
                vec![
                    arg(0, "url", "text", "Request URL"),
                    arg(1, "body", "any", "Request body"),
                    arg(2, "options", "dict", "Request options"),
                ],
                ret("dict", "Response with status, headers, body"),
                "Fails on network or timeout errors",
            ),
            op_err(
                "request",
                "http",
                "Performs a generic HTTP request with custom method",
                vec![
                    arg(0, "method", "text", "HTTP method"),
                    arg(1, "url", "text", "Request URL"),
                    arg(2, "body", "any", "Request body"),
                    arg(3, "options", "dict", "Request options"),
                ],
                ret("dict", "Response with status, headers, body"),
                "Fails on network or timeout errors",
            ),
        ],
    )
}

fn http_server_docs() -> StdlibNamespaceDoc {
    ns(
        "http.server",
        "HTTP server operations",
        vec![
            op_err(
                "listen",
                "http.server",
                "Starts listening on a port",
                vec![arg(0, "port", "long", "TCP port to listen on")],
                ret("http_server", "HTTP server handle"),
                "Fails if port is unavailable",
            ),
            op_err(
                "accept",
                "http.server",
                "Accepts the next incoming HTTP request",
                vec![arg(0, "handle", "http_server", "Server handle from listen")],
                ret(
                    "dict",
                    "Request object with method, path, headers, body, and connection handle",
                ),
                "Fails on I/O error",
            ),
            op_err(
                "respond",
                "http.server",
                "Sends an HTTP response to a connection",
                vec![
                    arg(0, "conn", "http_conn", "Connection handle from accept"),
                    arg(1, "status", "long", "HTTP status code"),
                    arg(2, "body", "text", "Response body"),
                ],
                ret("bool", "True on success"),
                "Fails on I/O error",
            ),
            op(
                "close",
                "http.server",
                "Closes a server handle",
                vec![arg(0, "handle", "http_server", "Server handle from listen")],
                ret("bool", "True on success"),
            ),
        ],
    )
}

fn http_respond_docs() -> StdlibNamespaceDoc {
    ns(
        "http.respond",
        "HTTP response convenience operations",
        vec![
            op_err(
                "html",
                "http.respond",
                "Sends an HTML response with content-type text/html",
                vec![
                    arg(0, "conn", "http_conn", "Connection handle from accept"),
                    arg(1, "status", "long", "HTTP status code"),
                    arg(2, "body", "text", "HTML response body"),
                    arg(3, "headers", "dict", "Optional extra response headers"),
                ],
                ret("bool", "True on success"),
                "Fails on I/O error",
            ),
            op_err(
                "json",
                "http.respond",
                "Sends a JSON response with content-type application/json",
                vec![
                    arg(0, "conn", "http_conn", "Connection handle from accept"),
                    arg(1, "status", "long", "HTTP status code"),
                    arg(2, "body", "text", "JSON response body"),
                    arg(3, "headers", "dict", "Optional extra response headers"),
                ],
                ret("bool", "True on success"),
                "Fails on I/O error",
            ),
            op_err(
                "text",
                "http.respond",
                "Sends a plain text response with content-type text/plain",
                vec![
                    arg(0, "conn", "http_conn", "Connection handle from accept"),
                    arg(1, "status", "long", "HTTP status code"),
                    arg(2, "body", "text", "Plain text response body"),
                    arg(3, "headers", "dict", "Optional extra response headers"),
                ],
                ret("bool", "True on success"),
                "Fails on I/O error",
            ),
            op_err(
                "file",
                "http.respond",
                "Sends a file as the response body with the given content-type. Reads the file as raw bytes, supporting both text and binary files (e.g. WASM, images)",
                vec![
                    arg(0, "conn", "http_conn", "Connection handle from accept"),
                    arg(1, "status", "long", "HTTP status code"),
                    arg(2, "path", "text", "File path to read and send"),
                    arg(3, "content_type", "text", "MIME content-type (e.g. application/wasm, application/javascript)"),
                ],
                ret("bool", "True on success"),
                "Fails on I/O or file-not-found error",
            ),
        ],
    )
}

fn ws_docs() -> StdlibNamespaceDoc {
    ns(
        "ws",
        "WebSocket client operations",
        vec![
            op_err(
                "connect",
                "ws",
                "Opens a WebSocket connection",
                vec![arg(0, "url", "text", "WebSocket URL (ws:// or wss://)")],
                ret("ws_conn", "WebSocket connection handle"),
                "Fails on connection error",
            ),
            op_err(
                "send",
                "ws",
                "Sends a text message on the WebSocket",
                vec![
                    arg(0, "handle", "ws_conn", "WebSocket connection handle"),
                    arg(1, "message", "text", "Message to send"),
                ],
                ret("bool", "True on success"),
                "Fails on I/O error",
            ),
            op_err(
                "recv",
                "ws",
                "Receives the next message from the WebSocket",
                vec![arg(0, "handle", "ws_conn", "WebSocket connection handle")],
                ret("text", "Received message text"),
                "Fails on I/O error or connection closed",
            ),
            op(
                "close",
                "ws",
                "Closes a WebSocket connection",
                vec![arg(0, "handle", "ws_conn", "WebSocket connection handle")],
                ret("bool", "True on success"),
            ),
        ],
    )
}

fn headers_docs() -> StdlibNamespaceDoc {
    ns(
        "headers",
        "HTTP header utilities",
        vec![
            op(
                "new",
                "headers",
                "Creates an empty headers dictionary",
                vec![],
                ret("dict", "Empty headers dictionary"),
            ),
            op(
                "set",
                "headers",
                "Returns headers with a header set",
                vec![
                    arg(0, "h", "dict", "Input headers"),
                    arg(1, "name", "text", "Header name"),
                    arg(2, "value", "text", "Header value"),
                ],
                ret("dict", "New headers with the header set"),
            ),
            op(
                "get",
                "headers",
                "Gets a header value by name",
                vec![
                    arg(0, "h", "dict", "Input headers"),
                    arg(1, "name", "text", "Header name"),
                ],
                ret("text", "Header value, or empty string if missing"),
            ),
            op(
                "delete",
                "headers",
                "Returns headers with a header removed",
                vec![
                    arg(0, "h", "dict", "Input headers"),
                    arg(1, "name", "text", "Header name to remove"),
                ],
                ret("dict", "New headers without the specified header"),
            ),
        ],
    )
}

fn auth_docs() -> StdlibNamespaceDoc {
    ns(
        "auth",
        "Authentication simulation helpers",
        vec![
            op(
                "extract_email",
                "auth",
                "Extracts email from parameters",
                vec![arg(0, "params", "dict", "Parameters dictionary")],
                ret("text", "Email value"),
            ),
            op(
                "extract_password",
                "auth",
                "Extracts password from parameters",
                vec![arg(0, "params", "dict", "Parameters dictionary")],
                ret("text", "Password value"),
            ),
            op(
                "validate_email",
                "auth",
                "Validates an email address format",
                vec![arg(0, "email", "text", "Email address")],
                ret("dict", "Validation result with valid and message fields"),
            ),
            op(
                "validate_password",
                "auth",
                "Validates a password meets requirements",
                vec![arg(0, "password", "text", "Password string")],
                ret("dict", "Validation result with valid and message fields"),
            ),
            op(
                "verify_password",
                "auth",
                "Verifies password against stored credentials",
                vec![
                    arg(0, "password", "text", "Password to verify"),
                    arg(1, "credentials", "dict", "Stored credentials"),
                ],
                ret("dict", "Verification result with valid field"),
            ),
            op(
                "sample_checks",
                "auth",
                "Returns sample security check results",
                vec![arg(0, "email", "text", "Email to check")],
                ret("list", "List of check result dictionaries"),
            ),
            op(
                "pass_through",
                "auth",
                "Passes a value through unchanged",
                vec![arg(0, "value", "any", "Any value")],
                ret("any", "Same value"),
            ),
        ],
    )
}

fn db_docs() -> StdlibNamespaceDoc {
    ns(
        "db",
        "SQLite database operations",
        vec![
            op_err(
                "open",
                "db",
                "Open or create a SQLite database (use \":memory:\" for in-memory)",
                vec![arg(0, "path", "text", "File path or \":memory:\"")],
                ret("db_conn", "Database connection handle"),
                "Fails if the database cannot be opened",
            ),
            op_err(
                "exec",
                "db",
                "Execute a non-SELECT statement (INSERT, UPDATE, DELETE, DDL)",
                vec![
                    arg(
                        0,
                        "conn",
                        "db_conn",
                        "Database connection handle from db.open",
                    ),
                    arg(1, "sql", "text", "SQL statement"),
                    arg(2, "params", "list", "Optional bind parameters"),
                ],
                ret("dict", "Dict with rows_affected count"),
                "Fails on SQL error or unknown handle",
            ),
            op_err(
                "query",
                "db",
                "Execute a SELECT and return rows as list of dicts",
                vec![
                    arg(
                        0,
                        "conn",
                        "db_conn",
                        "Database connection handle from db.open",
                    ),
                    arg(1, "sql", "text", "SQL SELECT statement"),
                    arg(2, "params", "list", "Optional bind parameters"),
                ],
                ret("list", "List of dicts keyed by column name"),
                "Fails on SQL error or unknown handle",
            ),
            op_err(
                "close",
                "db",
                "Close a database connection and release the handle",
                vec![arg(
                    0,
                    "conn",
                    "db_conn",
                    "Database connection handle to close",
                )],
                ret("bool", "true on success"),
                "Fails if handle is unknown",
            ),
            op(
                "query_user_by_email",
                "db",
                "(Legacy mock) Looks up a user by email address",
                vec![arg(0, "email", "text", "Email address to look up")],
                ret("dict", "User record or empty dictionary"),
            ),
            op(
                "query_credentials",
                "db",
                "(Legacy mock) Fetches credentials for an email",
                vec![arg(0, "email", "text", "Email address")],
                ret("dict", "Credentials record"),
            ),
        ],
    )
}

fn date_docs() -> StdlibNamespaceDoc {
    ns(
        "date",
        "Calendar date operations",
        vec![
            op(
                "now",
                "date",
                "Returns the current date in UTC",
                vec![],
                ret(
                    "dict",
                    "Date object with year, month, day, hour, minute, second fields",
                ),
            ),
            op(
                "now_tz",
                "date",
                "Returns the current date in a timezone",
                vec![arg(
                    0,
                    "tz",
                    "text",
                    "Timezone offset like +05:30 or -08:00",
                )],
                ret("dict", "Date object adjusted to timezone"),
            ),
            op(
                "from_unix_ms",
                "date",
                "Creates a date from Unix milliseconds",
                vec![arg(0, "ms", "long", "Unix timestamp in milliseconds")],
                ret("dict", "Date object"),
            ),
            op(
                "from_parts",
                "date",
                "Creates a date from year/month/day/hour/minute/second",
                vec![
                    arg(0, "year", "long", "Year"),
                    arg(1, "month", "long", "Month (1-12)"),
                    arg(2, "day", "long", "Day (1-31)"),
                    arg(3, "hour", "long", "Hour (0-23)"),
                    arg(4, "minute", "long", "Minute (0-59)"),
                    arg(5, "second", "long", "Second (0-59)"),
                ],
                ret("dict", "Date object"),
            ),
            op(
                "from_parts_tz",
                "date",
                "Creates a date from parts with timezone",
                vec![
                    arg(0, "year", "long", "Year"),
                    arg(1, "month", "long", "Month"),
                    arg(2, "day", "long", "Day"),
                    arg(3, "hour", "long", "Hour"),
                    arg(4, "minute", "long", "Minute"),
                    arg(5, "second", "long", "Second"),
                    arg(6, "tz", "text", "Timezone offset"),
                ],
                ret("dict", "Date object with timezone"),
            ),
            op_err(
                "from_iso",
                "date",
                "Parses an ISO 8601 date string",
                vec![arg(0, "s", "text", "ISO 8601 date string")],
                ret("dict", "Date object"),
                "Fails if string is not valid ISO 8601",
            ),
            op(
                "from_epoch",
                "date",
                "Creates a date from Unix epoch seconds",
                vec![arg(0, "seconds", "long", "Unix epoch seconds")],
                ret("dict", "Date object"),
            ),
            op(
                "to_unix_ms",
                "date",
                "Converts a date to Unix milliseconds",
                vec![arg(0, "date", "dict", "Date object")],
                ret("long", "Unix timestamp in milliseconds"),
            ),
            op(
                "to_parts",
                "date",
                "Extracts year/month/day/hour/minute/second from a date",
                vec![arg(0, "date", "dict", "Date object")],
                ret("dict", "Parts with year, month, day, hour, minute, second"),
            ),
            op(
                "to_iso",
                "date",
                "Formats a date as ISO 8601 string",
                vec![arg(0, "date", "dict", "Date object")],
                ret("text", "ISO 8601 formatted string"),
            ),
            op(
                "to_epoch",
                "date",
                "Converts a date to Unix epoch seconds",
                vec![arg(0, "date", "dict", "Date object")],
                ret("long", "Unix epoch seconds"),
            ),
            op(
                "weekday",
                "date",
                "Returns the day of week (0=Sun, 6=Sat)",
                vec![arg(0, "date", "dict", "Date object")],
                ret("long", "Weekday number"),
            ),
            op(
                "with_tz",
                "date",
                "Converts a date to a different timezone",
                vec![
                    arg(0, "date", "dict", "Date object"),
                    arg(1, "tz", "text", "Target timezone offset"),
                ],
                ret("dict", "Date object in new timezone"),
            ),
            op(
                "add",
                "date",
                "Adds a duration to a date",
                vec![
                    arg(0, "date", "dict", "Date object"),
                    arg(1, "amount", "long", "Amount to add"),
                    arg(
                        2,
                        "unit",
                        "text",
                        "Unit: years, months, days, hours, minutes, seconds",
                    ),
                ],
                ret("dict", "New date with duration added"),
            ),
            op(
                "add_days",
                "date",
                "Adds days to a date",
                vec![
                    arg(0, "date", "dict", "Date object"),
                    arg(1, "days", "long", "Number of days to add"),
                ],
                ret("dict", "New date"),
            ),
            op(
                "diff",
                "date",
                "Computes difference between two dates",
                vec![
                    arg(0, "a", "dict", "Start date"),
                    arg(1, "b", "dict", "End date"),
                    arg(2, "unit", "text", "Unit: days, hours, minutes, seconds"),
                ],
                ret("long", "Difference in the specified unit"),
            ),
            op(
                "compare",
                "date",
                "Compares two dates",
                vec![
                    arg(0, "a", "dict", "First date"),
                    arg(1, "b", "dict", "Second date"),
                ],
                ret("long", "-1 if a < b, 0 if equal, 1 if a > b"),
            ),
        ],
    )
}

fn stamp_docs() -> StdlibNamespaceDoc {
    ns(
        "stamp",
        "Monotonic timestamp operations",
        vec![
            op(
                "now",
                "stamp",
                "Returns the current timestamp in nanoseconds",
                vec![],
                ret("long", "Current timestamp as nanoseconds since epoch"),
            ),
            op(
                "from_ns",
                "stamp",
                "Creates a timestamp from nanoseconds",
                vec![arg(0, "ns", "long", "Nanoseconds since epoch")],
                ret("long", "Timestamp value"),
            ),
            op(
                "from_epoch",
                "stamp",
                "Creates a timestamp from epoch seconds",
                vec![arg(0, "seconds", "long", "Unix epoch seconds")],
                ret("long", "Timestamp in nanoseconds"),
            ),
            op(
                "to_ns",
                "stamp",
                "Converts a timestamp to nanoseconds",
                vec![arg(0, "ts", "long", "Timestamp")],
                ret("long", "Nanoseconds value"),
            ),
            op(
                "to_ms",
                "stamp",
                "Converts a timestamp to milliseconds",
                vec![arg(0, "ts", "long", "Timestamp in nanoseconds")],
                ret("long", "Milliseconds value"),
            ),
            op(
                "to_date",
                "stamp",
                "Converts a timestamp to a date object",
                vec![arg(0, "ts", "long", "Timestamp in nanoseconds")],
                ret("dict", "Date object"),
            ),
            op(
                "to_epoch",
                "stamp",
                "Converts a timestamp to epoch seconds",
                vec![arg(0, "ts", "long", "Timestamp in nanoseconds")],
                ret("long", "Unix epoch seconds"),
            ),
            op(
                "add",
                "stamp",
                "Adds milliseconds to a timestamp",
                vec![
                    arg(0, "ts", "long", "Timestamp in nanoseconds"),
                    arg(1, "ms", "long", "Milliseconds to add"),
                ],
                ret("long", "New timestamp"),
            ),
            op(
                "diff",
                "stamp",
                "Computes difference between two timestamps in milliseconds",
                vec![
                    arg(0, "a", "long", "Start timestamp"),
                    arg(1, "b", "long", "End timestamp"),
                ],
                ret("long", "Difference in milliseconds"),
            ),
            op(
                "compare",
                "stamp",
                "Compares two timestamps",
                vec![
                    arg(0, "a", "long", "First timestamp"),
                    arg(1, "b", "long", "Second timestamp"),
                ],
                ret("long", "-1 if a < b, 0 if equal, 1 if a > b"),
            ),
        ],
    )
}

fn trange_docs() -> StdlibNamespaceDoc {
    ns(
        "trange",
        "Time range operations",
        vec![
            op(
                "new",
                "trange",
                "Creates a time range from start and end timestamps",
                vec![
                    arg(0, "start", "long", "Start timestamp (nanoseconds)"),
                    arg(1, "end", "long", "End timestamp (nanoseconds)"),
                ],
                ret("dict", "Time range with start and end"),
            ),
            op(
                "start",
                "trange",
                "Returns the start of a time range",
                vec![arg(0, "range", "dict", "Time range")],
                ret("long", "Start timestamp"),
            ),
            op(
                "end",
                "trange",
                "Returns the end of a time range",
                vec![arg(0, "range", "dict", "Time range")],
                ret("long", "End timestamp"),
            ),
            op(
                "duration_ms",
                "trange",
                "Returns the duration in milliseconds",
                vec![arg(0, "range", "dict", "Time range")],
                ret("long", "Duration in milliseconds"),
            ),
            op(
                "contains",
                "trange",
                "Checks if a timestamp falls within the range",
                vec![
                    arg(0, "range", "dict", "Time range"),
                    arg(1, "ts", "long", "Timestamp to check"),
                ],
                ret("bool", "True if timestamp is within range"),
            ),
            op(
                "overlaps",
                "trange",
                "Checks if two time ranges overlap",
                vec![
                    arg(0, "a", "dict", "First time range"),
                    arg(1, "b", "dict", "Second time range"),
                ],
                ret("bool", "True if ranges overlap"),
            ),
            op(
                "shift",
                "trange",
                "Shifts a time range by milliseconds",
                vec![
                    arg(0, "range", "dict", "Time range"),
                    arg(1, "ms", "long", "Milliseconds to shift"),
                ],
                ret("dict", "Shifted time range"),
            ),
        ],
    )
}

fn time_docs() -> StdlibNamespaceDoc {
    ns(
        "time",
        "Time utility operations",
        vec![
            op(
                "sleep",
                "time",
                "Pauses execution for the given number of seconds",
                vec![arg(
                    0,
                    "seconds",
                    "real",
                    "Duration in seconds (e.g. 0.5 for 500ms)",
                )],
                ret("bool", "Always returns true after sleeping"),
            ),
            op(
                "tick",
                "time",
                "Waits for the next tick interval, then returns true. Alias for time.sleep, intended for use in source polling loops",
                vec![arg(
                    0,
                    "seconds",
                    "real",
                    "Tick interval in seconds (e.g. 3 for every 3 seconds)",
                )],
                ret("bool", "Always returns true after the tick interval"),
            ),
            op(
                "split_hms",
                "time",
                "Splits total hours into hours, minutes, seconds",
                vec![arg(0, "total_hours", "real", "Total hours as decimal")],
                ret("dict", "Object with hours, minutes, seconds fields"),
            ),
        ],
    )
}

fn fmt_docs() -> StdlibNamespaceDoc {
    ns(
        "fmt",
        "Formatting helper operations",
        vec![
            op(
                "pad_hms",
                "fmt",
                "Formats hours/minutes/seconds as zero-padded HH:MM:SS",
                vec![arg(0, "hms", "dict", "Object with hours, minutes, seconds")],
                ret("text", "Formatted time string like 02:30:45"),
            ),
            op(
                "wrap_field",
                "fmt",
                "Wraps a key-value pair into a dictionary",
                vec![
                    arg(0, "key", "text", "Field name"),
                    arg(1, "value", "any", "Field value"),
                ],
                ret("dict", "Single-key dictionary"),
            ),
        ],
    )
}

fn term_docs() -> StdlibNamespaceDoc {
    ns(
        "term",
        "Terminal I/O operations",
        vec![
            op(
                "print",
                "term",
                "Prints text to the terminal",
                vec![arg(0, "text", "text", "Text to print")],
                ret("void", "No return value"),
            ),
            op(
                "prompt",
                "term",
                "Prompts the user for text input",
                vec![arg(0, "message", "text", "Prompt message")],
                ret("text", "User input text"),
            ),
            op(
                "clear",
                "term",
                "Clears the terminal screen",
                vec![],
                ret("void", "No return value"),
            ),
            op(
                "size",
                "term",
                "Returns the terminal dimensions",
                vec![],
                ret("dict", "Object with cols and rows fields"),
            ),
            op(
                "cursor",
                "term",
                "Returns the current cursor position",
                vec![],
                ret("dict", "Object with col and row fields"),
            ),
            op(
                "move_to",
                "term",
                "Moves the cursor to a position",
                vec![
                    arg(0, "col", "long", "Column (0-based)"),
                    arg(1, "row", "long", "Row (0-based)"),
                ],
                ret("void", "No return value"),
            ),
            op(
                "color",
                "term",
                "Wraps text in ANSI color codes",
                vec![
                    arg(0, "text", "text", "Text to colorize"),
                    arg(
                        1,
                        "color",
                        "text",
                        "Color name: red, green, blue, yellow, cyan, magenta, white, bold, dim, reset",
                    ),
                ],
                ret("text", "Colorized text"),
            ),
            op(
                "read_key",
                "term",
                "Reads a single keypress from the terminal",
                vec![],
                ret("text", "Key name or character"),
            ),
        ],
    )
}

fn file_docs() -> StdlibNamespaceDoc {
    ns(
        "file",
        "File I/O operations",
        vec![
            op_err(
                "read",
                "file",
                "Reads a file's contents as text",
                vec![arg(0, "path", "text", "File path")],
                ret("text", "File contents"),
                "Fails if file does not exist or cannot be read",
            ),
            op_err(
                "write",
                "file",
                "Writes text to a file, creating or overwriting",
                vec![
                    arg(0, "path", "text", "File path"),
                    arg(1, "content", "text", "Content to write"),
                ],
                ret("bool", "True on success"),
                "Fails on I/O error",
            ),
            op_err(
                "append",
                "file",
                "Appends text to a file",
                vec![
                    arg(0, "path", "text", "File path"),
                    arg(1, "content", "text", "Content to append"),
                ],
                ret("bool", "True on success"),
                "Fails on I/O error",
            ),
            op_err(
                "delete",
                "file",
                "Deletes a file",
                vec![arg(0, "path", "text", "File path")],
                ret("bool", "True on success"),
                "Fails if file does not exist",
            ),
            op(
                "exists",
                "file",
                "Checks if a file or directory exists",
                vec![arg(0, "path", "text", "File path")],
                ret("bool", "True if path exists"),
            ),
            op_err(
                "list",
                "file",
                "Lists files and directories in a path",
                vec![arg(0, "path", "text", "Directory path")],
                ret("list", "List of entry names"),
                "Fails if path is not a directory",
            ),
            op_err(
                "mkdir",
                "file",
                "Creates a directory and parents",
                vec![arg(0, "path", "text", "Directory path")],
                ret("bool", "True on success"),
                "Fails on I/O error",
            ),
            op_err(
                "copy",
                "file",
                "Copies a file",
                vec![
                    arg(0, "src", "text", "Source path"),
                    arg(1, "dst", "text", "Destination path"),
                ],
                ret("bool", "True on success"),
                "Fails on I/O error",
            ),
            op_err(
                "move",
                "file",
                "Moves/renames a file",
                vec![
                    arg(0, "src", "text", "Source path"),
                    arg(1, "dst", "text", "Destination path"),
                ],
                ret("bool", "True on success"),
                "Fails on I/O error",
            ),
            op(
                "size",
                "file",
                "Returns the file size in bytes",
                vec![arg(0, "path", "text", "File path")],
                ret("long", "File size in bytes"),
            ),
            op(
                "is_dir",
                "file",
                "Checks if a path is a directory",
                vec![arg(0, "path", "text", "File path")],
                ret("bool", "True if path is a directory"),
            ),
        ],
    )
}

fn env_docs() -> StdlibNamespaceDoc {
    ns(
        "env",
        "Environment variable operations",
        vec![
            op(
                "get",
                "env",
                "Gets an environment variable value, with optional default",
                vec![
                    arg(0, "key", "text", "Variable name"),
                    arg(1, "default", "any", "Default if not set (optional)"),
                ],
                ret("text", "Variable value or default"),
            ),
            op(
                "set",
                "env",
                "Sets an environment variable",
                vec![
                    arg(0, "key", "text", "Variable name"),
                    arg(1, "value", "text", "Value to set"),
                ],
                ret("bool", "True on success"),
            ),
            op(
                "has",
                "env",
                "Checks if an environment variable is set",
                vec![arg(0, "key", "text", "Variable name")],
                ret("bool", "True if variable exists"),
            ),
            op(
                "list",
                "env",
                "Returns all environment variables as a dict",
                vec![],
                ret("dict", "All environment variables"),
            ),
            op(
                "remove",
                "env",
                "Removes an environment variable",
                vec![arg(0, "key", "text", "Variable name")],
                ret("bool", "True on success"),
            ),
        ],
    )
}

fn exec_docs() -> StdlibNamespaceDoc {
    ns(
        "exec",
        "Process execution operations",
        vec![op_err(
            "run",
            "exec",
            "Runs an external command and captures output",
            vec![
                arg(0, "command", "text", "Command to execute"),
                arg(1, "args", "list", "Command arguments (optional)"),
            ],
            ret("dict", "Result with code, stdout, stderr, ok fields"),
            "Fails if command cannot be started",
        )],
    )
}

fn regex_docs() -> StdlibNamespaceDoc {
    ns(
        "regex",
        "Regular expression operations",
        vec![
            op_err(
                "match",
                "regex",
                "Tests if text matches a regex pattern",
                vec![
                    arg(0, "text", "text", "Input text"),
                    arg(1, "pattern", "text", "Regex pattern"),
                ],
                ret("bool", "True if text matches"),
                "Fails on invalid regex",
            ),
            op_err(
                "find",
                "regex",
                "Finds first match with capture groups",
                vec![
                    arg(0, "text", "text", "Input text"),
                    arg(1, "pattern", "text", "Regex pattern"),
                ],
                ret("dict", "Object with matched, text, and groups fields"),
                "Fails on invalid regex",
            ),
            op_err(
                "find_all",
                "regex",
                "Finds all non-overlapping matches",
                vec![
                    arg(0, "text", "text", "Input text"),
                    arg(1, "pattern", "text", "Regex pattern"),
                ],
                ret("list", "List of matched strings"),
                "Fails on invalid regex",
            ),
            op_err(
                "replace",
                "regex",
                "Replaces first match with replacement text",
                vec![
                    arg(0, "text", "text", "Input text"),
                    arg(1, "pattern", "text", "Regex pattern"),
                    arg(2, "replacement", "text", "Replacement string"),
                ],
                ret("text", "Text with first match replaced"),
                "Fails on invalid regex",
            ),
            op_err(
                "replace_all",
                "regex",
                "Replaces all matches with replacement text",
                vec![
                    arg(0, "text", "text", "Input text"),
                    arg(1, "pattern", "text", "Regex pattern"),
                    arg(2, "replacement", "text", "Replacement string"),
                ],
                ret("text", "Text with all matches replaced"),
                "Fails on invalid regex",
            ),
            op_err(
                "split",
                "regex",
                "Splits text by a regex pattern",
                vec![
                    arg(0, "text", "text", "Input text"),
                    arg(1, "pattern", "text", "Regex pattern"),
                ],
                ret("list", "List of split segments"),
                "Fails on invalid regex",
            ),
        ],
    )
}

fn random_docs() -> StdlibNamespaceDoc {
    ns(
        "random",
        "Random number and UUID generation",
        vec![
            op_err(
                "int",
                "random",
                "Generates a random integer in [min, max] inclusive",
                vec![
                    arg(0, "min", "long", "Minimum value"),
                    arg(1, "max", "long", "Maximum value"),
                ],
                ret("long", "Random integer"),
                "Fails if min > max",
            ),
            op(
                "float",
                "random",
                "Generates a random float in [0.0, 1.0)",
                vec![],
                ret("real", "Random float"),
            ),
            op(
                "uuid",
                "random",
                "Generates a random UUID v4",
                vec![],
                ret("text", "UUID string"),
            ),
            op_err(
                "choice",
                "random",
                "Picks a random element from a list",
                vec![arg(0, "items", "list", "Non-empty list")],
                ret("any", "Randomly selected element"),
                "Fails if list is empty",
            ),
            op(
                "shuffle",
                "random",
                "Returns a shuffled copy of a list",
                vec![arg(0, "items", "list", "List to shuffle")],
                ret("list", "Shuffled list"),
            ),
        ],
    )
}

fn hash_docs() -> StdlibNamespaceDoc {
    ns(
        "hash",
        "Cryptographic hash digests",
        vec![
            op(
                "sha256",
                "hash",
                "Computes SHA-256 digest of input text",
                vec![arg(0, "data", "text", "Input data to hash")],
                ret("text", "64-character hex-encoded SHA-256 digest"),
            ),
            op(
                "sha512",
                "hash",
                "Computes SHA-512 digest of input text",
                vec![arg(0, "data", "text", "Input data to hash")],
                ret("text", "128-character hex-encoded SHA-512 digest"),
            ),
            op_err(
                "hmac",
                "hash",
                "Computes HMAC of input text with a key",
                vec![
                    arg(0, "data", "text", "Input data"),
                    arg(1, "key", "text", "Secret key"),
                    arg(2, "algo", "text", "Algorithm: sha256 (default) or sha512"),
                ],
                ret("text", "Hex-encoded HMAC"),
                "Fails if algorithm is not sha256 or sha512",
            ),
        ],
    )
}

fn base64_docs() -> StdlibNamespaceDoc {
    ns(
        "base64",
        "Base64 encoding and decoding",
        vec![
            op(
                "encode",
                "base64",
                "Encodes text to standard base64 with padding",
                vec![arg(0, "data", "text", "Input text")],
                ret("text", "Base64-encoded string"),
            ),
            op_err(
                "decode",
                "base64",
                "Decodes standard base64 to text",
                vec![arg(0, "encoded", "text", "Base64-encoded string")],
                ret("text", "Decoded UTF-8 text"),
                "Fails if input is not valid base64 or decoded bytes are not valid UTF-8",
            ),
            op(
                "encode_url",
                "base64",
                "Encodes text to URL-safe base64 without padding",
                vec![arg(0, "data", "text", "Input text")],
                ret("text", "URL-safe base64-encoded string"),
            ),
            op_err(
                "decode_url",
                "base64",
                "Decodes URL-safe base64 to text",
                vec![arg(0, "encoded", "text", "URL-safe base64-encoded string")],
                ret("text", "Decoded UTF-8 text"),
                "Fails if input is not valid URL-safe base64 or decoded bytes are not valid UTF-8",
            ),
        ],
    )
}

fn crypto_docs() -> StdlibNamespaceDoc {
    ns(
        "crypto",
        "Password hashing, JWT tokens, and secure random bytes",
        vec![
            op(
                "hash_password",
                "crypto",
                "Hashes a password using bcrypt (cost 12)",
                vec![arg(0, "password", "text", "Password to hash")],
                ret("text", "Bcrypt hash string"),
            ),
            op(
                "verify_password",
                "crypto",
                "Verifies a password against a bcrypt hash",
                vec![
                    arg(0, "password", "text", "Password to verify"),
                    arg(1, "hash", "text", "Bcrypt hash to check against"),
                ],
                ret("bool", "True if password matches the hash"),
            ),
            op(
                "sign_token",
                "crypto",
                "Creates an HS256 JWT-format signed token",
                vec![
                    arg(0, "payload", "dict", "Claims payload"),
                    arg(1, "secret", "text", "Signing secret"),
                ],
                ret("text", "Signed JWT string"),
            ),
            op(
                "verify_token",
                "crypto",
                "Verifies an HS256 JWT token and extracts payload",
                vec![
                    arg(0, "token", "text", "JWT string"),
                    arg(1, "secret", "text", "Signing secret"),
                ],
                ret("dict", "Dict with valid (bool) and payload or error fields"),
            ),
            op_err(
                "random_bytes",
                "crypto",
                "Generates cryptographically secure random bytes",
                vec![arg(0, "count", "long", "Number of bytes (1-1024)")],
                ret("text", "Hex-encoded random bytes"),
                "Fails if count is not between 1 and 1024",
            ),
        ],
    )
}

fn log_docs() -> StdlibNamespaceDoc {
    ns(
        "log",
        "Level-based logging to stderr with timestamps",
        vec![
            op(
                "debug",
                "log",
                "Log a debug-level message",
                vec![
                    arg(0, "message", "text", "Log message"),
                    arg(1, "context", "dict", "Optional structured context"),
                ],
                ret("bool", "true"),
            ),
            op(
                "info",
                "log",
                "Log an info-level message",
                vec![
                    arg(0, "message", "text", "Log message"),
                    arg(1, "context", "dict", "Optional structured context"),
                ],
                ret("bool", "true"),
            ),
            op(
                "warn",
                "log",
                "Log a warn-level message",
                vec![
                    arg(0, "message", "text", "Log message"),
                    arg(1, "context", "dict", "Optional structured context"),
                ],
                ret("bool", "true"),
            ),
            op(
                "error",
                "log",
                "Log an error-level message",
                vec![
                    arg(0, "message", "text", "Log message"),
                    arg(1, "context", "dict", "Optional structured context"),
                ],
                ret("bool", "true"),
            ),
            op(
                "trace",
                "log",
                "Log a trace-level message",
                vec![
                    arg(0, "message", "text", "Log message"),
                    arg(1, "context", "dict", "Optional structured context"),
                ],
                ret("bool", "true"),
            ),
        ],
    )
}

fn error_docs() -> StdlibNamespaceDoc {
    ns(
        "error",
        "Structured error construction and inspection",
        vec![
            op(
                "new",
                "error",
                "Create a structured error dict with code and message",
                vec![
                    arg(0, "code", "text", "Error code (e.g. \"NOT_FOUND\")"),
                    arg(1, "message", "text", "Human-readable error message"),
                    arg(2, "details", "dict", "Optional additional context"),
                ],
                ret(
                    "dict",
                    "Error dict with code, message, and optional details",
                ),
            ),
            op(
                "wrap",
                "error",
                "Add context prefix to an existing error's message",
                vec![
                    arg(0, "error", "dict", "Existing error dict"),
                    arg(1, "context", "text", "Context string to prepend"),
                ],
                ret("dict", "Error dict with updated message"),
            ),
            op(
                "code",
                "error",
                "Extract the code field from an error dict",
                vec![arg(0, "error", "dict", "Error dict")],
                ret("text", "The error code, or null if absent"),
            ),
            op(
                "message",
                "error",
                "Extract the message field from an error dict",
                vec![arg(0, "error", "dict", "Error dict")],
                ret("text", "The error message, or null if absent"),
            ),
        ],
    )
}

fn cookie_docs() -> StdlibNamespaceDoc {
    ns(
        "cookie",
        "HTTP cookie parsing, construction, and deletion",
        vec![
            op(
                "parse",
                "cookie",
                "Parse a Cookie header string into a dict of name-value pairs",
                vec![arg(
                    0,
                    "header",
                    "text",
                    "Cookie header string (e.g. \"session=abc; theme=dark\")",
                )],
                ret("dict", "Dict mapping cookie names to values"),
            ),
            op(
                "get",
                "cookie",
                "Look up a cookie value by name from a parsed cookie dict",
                vec![
                    arg(0, "cookies", "dict", "Parsed cookie dict"),
                    arg(1, "name", "text", "Cookie name to look up"),
                ],
                ret("text", "The cookie value, or null if not found"),
            ),
            op(
                "set",
                "cookie",
                "Build a Set-Cookie header string with optional attributes",
                vec![
                    arg(0, "name", "text", "Cookie name"),
                    arg(1, "value", "text", "Cookie value"),
                    arg(
                        2,
                        "opts",
                        "dict",
                        "Optional attributes: path, domain, max_age, secure, http_only, same_site",
                    ),
                ],
                ret("text", "Set-Cookie header string"),
            ),
            op(
                "delete",
                "cookie",
                "Build a Set-Cookie header that deletes a cookie (Max-Age=0)",
                vec![arg(0, "name", "text", "Cookie name to delete")],
                ret("text", "Set-Cookie header string with Max-Age=0"),
            ),
        ],
    )
}

fn url_docs() -> StdlibNamespaceDoc {
    ns(
        "url",
        "URL parsing, query string handling, and percent-encoding",
        vec![
            op(
                "parse",
                "url",
                "Decompose a URL into path, query, and fragment components",
                vec![arg(0, "url", "text", "URL string to parse")],
                ret("dict", "Dict with path, query, and fragment fields"),
            ),
            op(
                "query_parse",
                "url",
                "Parse a query string into a dict with percent-decoding",
                vec![arg(
                    0,
                    "qs",
                    "text",
                    "Query string (e.g. \"a=1&b=hello%20world\")",
                )],
                ret("dict", "Dict mapping parameter names to decoded values"),
            ),
            op(
                "encode",
                "url",
                "Percent-encode a string per RFC 3986",
                vec![arg(0, "text", "text", "String to encode")],
                ret("text", "Percent-encoded string"),
            ),
            op(
                "decode",
                "url",
                "Percent-decode a string (handles + as space)",
                vec![arg(0, "text", "text", "Percent-encoded string to decode")],
                ret("text", "Decoded string"),
            ),
        ],
    )
}

fn route_docs() -> StdlibNamespaceDoc {
    ns(
        "route",
        "URL path pattern matching with named parameters and wildcards",
        vec![
            op(
                "match",
                "route",
                "Test whether a URL path matches a pattern with :param and *wildcard segments",
                vec![
                    arg(
                        0,
                        "pattern",
                        "text",
                        "Route pattern (e.g. \"/users/:id/posts/*rest\")",
                    ),
                    arg(1, "path", "text", "URL path to match against"),
                ],
                ret("bool", "True if the path matches the pattern"),
            ),
            op(
                "params",
                "route",
                "Extract named parameters from a URL path given a pattern",
                vec![
                    arg(
                        0,
                        "pattern",
                        "text",
                        "Route pattern (e.g. \"/users/:id\")",
                    ),
                    arg(1, "path", "text", "URL path to extract params from"),
                ],
                ret(
                    "dict",
                    "Dict of captured parameter values (empty dict if no match)",
                ),
            ),
        ],
    )
}

fn html_docs() -> StdlibNamespaceDoc {
    ns(
        "html",
        "HTML entity escaping and unescaping",
        vec![
            op(
                "escape",
                "html",
                "Escape HTML special characters (& < > \" ')",
                vec![arg(0, "text", "text", "String to escape")],
                ret("text", "HTML-escaped string"),
            ),
            op(
                "unescape",
                "html",
                "Decode HTML entities back to characters",
                vec![arg(0, "text", "text", "HTML-escaped string to decode")],
                ret("text", "Unescaped string"),
            ),
        ],
    )
}

fn tmpl_docs() -> StdlibNamespaceDoc {
    ns(
        "tmpl",
        "Mustache-style template rendering",
        vec![op(
            "render",
            "tmpl",
            "Render a Mustache template with data. Supports {{key}} (escaped), {{{key}}} (raw), {{#section}}...{{/section}} (truthy/list), {{^section}}...{{/section}} (inverted), and {{.}} (current item)",
            vec![
                arg(0, "template", "text", "Mustache template string"),
                arg(1, "data", "dict", "Data dict for variable substitution"),
            ],
            ret("text", "Rendered text output"),
        )],
    )
}

fn ffi_docs() -> StdlibNamespaceDoc {
    ns(
        "ffi",
        "Foreign function interface for calling external C libraries",
        vec![op(
            "available",
            "ffi",
            "Check whether an FFI library is available on this system",
            vec![arg(0, "lib_name", "text", "Library name to check availability for")],
            ret("bool", "true if the library can be loaded, false otherwise"),
        )],
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime;
    use std::collections::HashSet;

    #[test]
    fn stdlib_docs_cover_all_known_ops() {
        use crate::codec::CodecRegistry;

        let mut known: HashSet<String> =
            runtime::known_ops().iter().map(|s| s.to_string()).collect();
        // Internal ops generated by flow lowering, not user-facing
        known.remove("accept");
        let codec_registry = CodecRegistry::default_registry();
        for op in codec_registry.known_ops() {
            known.insert(op);
        }

        let docs = all_stdlib_docs();
        let documented: HashSet<String> = docs
            .iter()
            .flat_map(|ns| ns.ops.iter().map(|op| op.full_name.clone()))
            .collect();

        let mut missing: Vec<&String> = known
            .iter()
            .filter(|op| !documented.contains(op.as_str()))
            .collect();
        missing.sort();

        let mut extra: Vec<&String> = documented
            .iter()
            .filter(|op| !known.contains(op.as_str()))
            .collect();
        extra.sort();

        assert!(
            missing.is_empty(),
            "Ops in known_ops() but not in stdlib_docs: {missing:?}"
        );
        assert!(
            extra.is_empty(),
            "Ops in stdlib_docs but not in known_ops(): {extra:?}"
        );
    }

    #[test]
    fn all_ops_have_valid_full_names() {
        let docs = all_stdlib_docs();
        for ns_doc in &docs {
            for op_doc in &ns_doc.ops {
                let expected = format!("{}.{}", ns_doc.namespace, op_doc.name);
                assert_eq!(
                    op_doc.full_name, expected,
                    "full_name mismatch for {}: expected {expected}",
                    op_doc.full_name
                );
            }
        }
    }
}
