import { StreamLanguage, LanguageSupport } from "@codemirror/language";
import { tags as t } from "@lezer/highlight";

const keywords = new Set([
  "func", "flow", "source", "sink", "docs", "test", "type", "data", "enum",
  "take", "emit", "fail", "return", "as", "body", "done", "use", "from",
  "step", "branch", "when", "case", "if", "else", "loop", "sync", "on",
  "send", "nowait", "state", "must", "mock", "trap", "next", "to", "open",
  "break",
]);

const builtinTypes = new Set([
  "text", "bool", "long", "real", "uuid", "time", "list", "dict", "void",
  "db_conn", "http_server", "http_conn", "ws_conn",
]);

const booleans = new Set(["true", "false"]);

function tokenBase(stream: any, state: any): string | null {
  // Comments
  if (stream.match("#") && !state.inString) {
    // Check for interpolation inside string
    if (state.inString) {
      if (stream.match("{")) {
        state.interpDepth = (state.interpDepth || 0) + 1;
        return "brace";
      }
    }
    stream.skipToEnd();
    return "comment";
  }

  // Triple-quoted strings
  if (stream.match('"""')) {
    if (state.inTripleString) {
      state.inTripleString = false;
      return "string";
    }
    state.inTripleString = true;
    return "string";
  }

  if (state.inTripleString) {
    if (stream.match("#{")) {
      state.interpDepth = (state.interpDepth || 0) + 1;
      return "brace";
    }
    while (stream.next()) {
      if (stream.match('"""', false)) break;
      if (stream.match("#{", false)) break;
    }
    return "string";
  }

  // Strings
  if (stream.match('"')) {
    state.inString = !state.inString;
    return "string";
  }

  if (state.inString) {
    if (stream.match("\\\\") || stream.match('\\"') || stream.match("\\n") ||
        stream.match("\\t") || stream.match("\\#")) {
      return "escape";
    }
    if (stream.match("#{")) {
      state.interpDepth = (state.interpDepth || 0) + 1;
      return "brace";
    }
    if (stream.next() === '"') {
      state.inString = false;
      return "string";
    }
    return "string";
  }

  // Close interpolation
  if (state.interpDepth && stream.match("}")) {
    state.interpDepth--;
    if (state.interpDepth === 0 && state.inString) {
      // Back to string mode
    }
    return "brace";
  }

  // Numbers
  if (stream.match(/^-?\d+(\.\d+)?/)) return "number";

  // Fat arrow
  if (stream.match("=>")) return "punctuation";

  // Operators
  if (stream.match("**") || stream.match("==") || stream.match("!=") ||
      stream.match("<=") || stream.match(">=") || stream.match("&&") ||
      stream.match("||")) return "operator";
  if (stream.match(/^[+\-*/%<>=!]/)) return "operator";

  // Symbols (:name)
  if (stream.match(/^:[a-zA-Z_]\w*/)) return "atom";

  // Identifiers and keywords
  if (stream.match(/^[a-zA-Z_]\w*/)) {
    const word = stream.current();
    if (keywords.has(word)) return "keyword";
    if (builtinTypes.has(word)) return "typeName";
    if (booleans.has(word)) return "bool";
    // Check for namespace call (word.word)
    if (stream.match(/^\.\w+/, false)) return "variableName.definition";
    return "variableName";
  }

  // Brackets
  if (stream.match(/^[(){}\[\]]/)) return "bracket";

  // Punctuation
  if (stream.match(/^[,.:?]/)) return "punctuation";

  stream.next();
  return null;
}

const foraiStreamParser = {
  startState() {
    return {
      inString: false,
      inTripleString: false,
      interpDepth: 0,
    };
  },
  token: tokenBase,
  languageData: {
    commentTokens: { line: "#" },
    closeBrackets: { brackets: ["(", "[", "{", '"'] },
    indentOnInput: /^\s*(done|else|when)\b/,
  },
};

const foraiLanguage = StreamLanguage.define(foraiStreamParser);

export function forai() {
  return new LanguageSupport(foraiLanguage);
}
