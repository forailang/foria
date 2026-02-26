import { EditorState } from "@codemirror/state";
import { EditorView, keymap, lineNumbers, highlightActiveLine, highlightActiveLineGutter } from "@codemirror/view";
import { defaultKeymap, indentWithTab, history, historyKeymap } from "@codemirror/commands";
import { bracketMatching, indentOnInput } from "@codemirror/language";
import { closeBrackets, closeBracketsKeymap } from "@codemirror/autocomplete";
import { linter, Diagnostic } from "@codemirror/lint";
import { searchKeymap, highlightSelectionMatches } from "@codemirror/search";
import { forai } from "./lang/forai";
import type { CompileError, IrData, WorkerResponse } from "./types";

// --- State ---

interface FileState {
  name: string;
  source: string;
}

let files: FileState[] = [
  {
    name: "main.fa",
    source: `use lib from "./lib"

docs main
    Pipeline example with greeting.
done

flow main
    emit result as text
    fail error as text
body
    step lib.Greet("world" to :name) then
        next :result to msg
    done
    emit msg to :result
done

test main
    mock lib.Greet => "Hello world!"
    result = main()
    must result == "Hello world!"
done
`,
  },
  {
    name: "lib/Greet.fa",
    source: `docs Greet
    Builds a greeting string.
done

func Greet
    take name as text
    emit result as text
    fail error as text
body
    greeting = "Hello #{name}!"
    emit greeting
done

test Greet
    result = Greet("world")
    must result == "Hello world!"
done
`,
  },
];

let activeFileIndex = 0;
let compileErrors: CompileError[] = [];
let lastIrData: IrData | null = null;
let compilerReady = false;
let compileTimer: ReturnType<typeof setTimeout> | null = null;
let editorView: EditorView | null = null;

// --- Compiler Worker ---

const compilerWorker = new Worker("./compiler-worker.js", { type: "module" });
let compileId = 0;

compilerWorker.onmessage = (e: MessageEvent<WorkerResponse>) => {
  const msg = e.data;

  if (msg.type === "ready") {
    compilerReady = true;
    setStatus("Ready");
    hideLoading();
    triggerCompile();
  }

  if (msg.type === "compile-result") {
    if (msg.id !== compileId) return; // Discard stale results
    if (msg.error) {
      setCompileStatus(`Error: ${msg.error}`);
      return;
    }
    const result = msg.result;
    const elapsed = msg.elapsed ?? 0;
    if (result?.ok) {
      compileErrors = [];
      lastIrData = result.ok.entry_ir;
      setCompileStatus(`Compiled in ${elapsed.toFixed(0)}ms`);
    } else if (result?.errors) {
      compileErrors = result.errors;
      lastIrData = null;
      setCompileStatus(`${result.errors.length} error(s)`);
    }
    updateLintDiagnostics();
    updateOutputPanel();
  }

  if (msg.type === "format-result") {
    if (msg.formatted && editorView) {
      files[activeFileIndex].source = msg.formatted;
      editorView.dispatch({
        changes: {
          from: 0,
          to: editorView.state.doc.length,
          insert: msg.formatted,
        },
      });
    }
  }

  if (msg.type === "error") {
    setStatus(`Error: ${msg.message}`);
  }
};

function triggerCompile() {
  if (!compilerReady) return;
  if (compileTimer) clearTimeout(compileTimer);
  compileTimer = setTimeout(() => {
    const fileMap: Record<string, string> = {};
    for (const f of files) {
      fileMap[f.name] = f.source;
    }
    compileId++;
    compilerWorker.postMessage({
      type: "compile",
      id: compileId,
      files: fileMap,
      entryPoint: "main.fa",
    });
    setCompileStatus("Compiling...");
  }, 300);
}

// --- Editor ---

const darkTheme = EditorView.theme({
  "&": { backgroundColor: "#0d1117", color: "#c9d1d9" },
  ".cm-content": { caretColor: "#58a6ff" },
  ".cm-cursor": { borderLeftColor: "#58a6ff" },
  "&.cm-focused .cm-selectionBackground, ::selection": { backgroundColor: "#1f6feb40" },
  ".cm-gutters": { backgroundColor: "#0d1117", color: "#484f58", border: "none" },
  ".cm-activeLineGutter": { backgroundColor: "#161b2280" },
  ".cm-activeLine": { backgroundColor: "#161b2280" },
  ".cm-matchingBracket": { backgroundColor: "#3fb95040", outline: "1px solid #3fb95060" },
}, { dark: true });

function createEditorState(doc: string): EditorState {
  return EditorState.create({
    doc,
    extensions: [
      lineNumbers(),
      highlightActiveLine(),
      highlightActiveLineGutter(),
      history(),
      bracketMatching(),
      closeBrackets(),
      indentOnInput(),
      highlightSelectionMatches(),
      forai(),
      darkTheme,
      keymap.of([
        ...defaultKeymap,
        ...historyKeymap,
        ...closeBracketsKeymap,
        ...searchKeymap,
        indentWithTab,
        { key: "Mod-Enter", run: () => { handleRun(); return true; } },
      ]),
      EditorView.updateListener.of((update) => {
        if (update.docChanged) {
          files[activeFileIndex].source = update.state.doc.toString();
          triggerCompile();
        }
      }),
      linter(() => {
        const diagnostics: Diagnostic[] = [];
        const activeFile = files[activeFileIndex].name;
        for (const err of compileErrors) {
          if (err.file === activeFile && err.line > 0) {
            const line = editorView?.state.doc.line(Math.min(err.line, editorView.state.doc.lines));
            if (line) {
              diagnostics.push({
                from: line.from + Math.max(0, (err.col || 1) - 1),
                to: line.to,
                severity: "error",
                message: err.message,
              });
            }
          }
        }
        return diagnostics;
      }),
    ],
  });
}

function initEditor() {
  const container = document.getElementById("editor-area")!;
  editorView = new EditorView({
    state: createEditorState(files[activeFileIndex].source),
    parent: container,
  });
}

function switchFile(index: number) {
  if (index === activeFileIndex) return;
  // Save current
  if (editorView) {
    files[activeFileIndex].source = editorView.state.doc.toString();
  }
  activeFileIndex = index;
  if (editorView) {
    editorView.setState(createEditorState(files[index].source));
  }
  renderTabs();
}

// --- Tabs ---

function renderTabs() {
  const tabBar = document.getElementById("editor-tabs")!;
  tabBar.innerHTML = "";
  files.forEach((f, i) => {
    const tab = document.createElement("div");
    tab.className = `tab${i === activeFileIndex ? " active" : ""}`;
    const span = document.createElement("span");
    span.textContent = f.name;
    tab.appendChild(span);
    tab.onclick = () => switchFile(i);
    tabBar.appendChild(tab);
  });
}

// --- Output Panel ---

let activeOutputTab = "output";
let outputLines: string[] = [];

function updateOutputPanel() {
  const panel = document.getElementById("output-panel")!;

  if (activeOutputTab === "output") {
    if (outputLines.length === 0) {
      panel.innerHTML = '<div class="empty-state">Press Run to execute your code</div>';
    } else {
      panel.innerHTML = outputLines
        .map((l) => `<div class="log-line">${escapeHtml(l)}</div>`)
        .join("");
    }
  } else if (activeOutputTab === "ir-json") {
    if (lastIrData) {
      const json = JSON.stringify(lastIrData, null, 2);
      panel.innerHTML = `<pre class="ir-json">${escapeHtml(json)}</pre>`;
    } else {
      panel.innerHTML = '<div class="empty-state">No IR available</div>';
    }
  } else if (activeOutputTab === "errors") {
    if (compileErrors.length === 0) {
      panel.innerHTML = '<div class="empty-state">No errors</div>';
    } else {
      panel.innerHTML = compileErrors
        .map(
          (e) =>
            `<div class="compile-error">${escapeHtml(e.file)}:${e.line}:${e.col} ${escapeHtml(e.message)}</div>`
        )
        .join("");
    }
  }
}

function initOutputTabs() {
  document.querySelectorAll(".output-tab").forEach((tab) => {
    (tab as HTMLElement).onclick = () => {
      document.querySelectorAll(".output-tab").forEach((t) => t.classList.remove("active"));
      tab.classList.add("active");
      activeOutputTab = (tab as HTMLElement).dataset.panel || "output";
      updateOutputPanel();
    };
  });
}

// --- Run ---

function handleRun() {
  outputLines = [];
  setStatus("Running...");

  // For now, compilation output only — runtime Worker integration is Phase 2+
  const fileMap: Record<string, string> = {};
  for (const f of files) {
    fileMap[f.name] = f.source;
  }

  compilerWorker.postMessage({
    type: "compile",
    id: ++compileId,
    files: fileMap,
    entryPoint: "main.fa",
  });

  if (compileErrors.length === 0 && lastIrData) {
    outputLines.push("Compilation successful.");
    outputLines.push(`IR has ${lastIrData.nodes?.length || 0} nodes.`);
    outputLines.push("");
    outputLines.push("(Runtime execution coming soon)");
  } else {
    outputLines.push("Compilation failed. See Errors tab.");
  }

  activeOutputTab = "output";
  document.querySelectorAll(".output-tab").forEach((t) => t.classList.remove("active"));
  document.querySelector('.output-tab[data-panel="output"]')?.classList.add("active");
  updateOutputPanel();
  setStatus("Ready");
}

// --- Format ---

function handleFormat() {
  if (!compilerReady || !editorView) return;
  compilerWorker.postMessage({
    type: "format",
    id: ++compileId,
    source: editorView.state.doc.toString(),
  });
}

// --- Resize ---

function initResize() {
  const handle = document.getElementById("resize-handle")!;
  const editorPanel = document.getElementById("panel-editor")!;
  let startX = 0;
  let startWidth = 0;

  handle.onmousedown = (e: MouseEvent) => {
    startX = e.clientX;
    startWidth = editorPanel.offsetWidth;
    document.onmousemove = (e: MouseEvent) => {
      const diff = e.clientX - startX;
      editorPanel.style.flex = "none";
      editorPanel.style.width = `${Math.max(200, startWidth + diff)}px`;
    };
    document.onmouseup = () => {
      document.onmousemove = null;
      document.onmouseup = null;
    };
    e.preventDefault();
  };
}

// --- Helpers ---

function setStatus(text: string) {
  document.getElementById("status")!.textContent = text;
}

function setCompileStatus(text: string) {
  document.getElementById("compile-status")!.textContent = text;
}

function hideLoading() {
  document.getElementById("loading")!.classList.add("hidden");
}

function updateLintDiagnostics() {
  // Force re-lint by dispatching an empty transaction
  if (editorView) {
    editorView.dispatch({});
  }
}

function escapeHtml(s: string): string {
  return s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
}

// --- Init ---

function init() {
  initEditor();
  renderTabs();
  initOutputTabs();
  initResize();

  document.getElementById("btn-run")!.onclick = handleRun;
  document.getElementById("btn-format")!.onclick = handleFormat;

  // Detect platform for keyboard shortcut display
  const isMac = /Mac|iPod|iPhone|iPad/.test(navigator.platform);
  document.getElementById("run-shortcut")!.textContent = isMac ? "Cmd+Enter" : "Ctrl+Enter";

  // Update file count
  document.getElementById("file-count")!.textContent = `${files.length} file(s)`;
}

init();
