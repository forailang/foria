import { EditorState } from "@codemirror/state";
import { EditorView, keymap, lineNumbers, highlightActiveLine, highlightActiveLineGutter } from "@codemirror/view";
import { defaultKeymap, indentWithTab, history, historyKeymap } from "@codemirror/commands";
import { bracketMatching, indentOnInput } from "@codemirror/language";
import { closeBrackets, closeBracketsKeymap } from "@codemirror/autocomplete";
import { linter, Diagnostic } from "@codemirror/lint";
import { searchKeymap, highlightSelectionMatches } from "@codemirror/search";
import { forai } from "./lang/forai";
import { renderGraph } from "./graph";
import type { CompileError, IrData, StepSnapshot, WorkerResponse } from "./types";
import type { GraphState } from "./graph";

// --- Constants ---

const AUTOSAVE_KEY = "forai-playground-state";
const AUTOSAVE_DEBOUNCE_MS = 1000;
const OUTPUT_MAX_LINES = 10000;
const EXECUTE_TIMEOUT_MS = 10000;

// --- Examples ---

interface Example {
  name: string;
  files: FileState[];
}

const EXAMPLES: Example[] = [
  {
    name: "Hello World",
    files: [
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
    _ = term.print(greeting)
    emit greeting
done

test Greet
    mock term.print => true
    result = Greet("world")
    must result == "Hello world!"
done
`,
      },
    ],
  },
  {
    name: "FizzBuzz",
    files: [
      {
        name: "main.fa",
        source: `use lib from "./lib"

docs main
    Classic FizzBuzz from 1 to 30.
done

flow main
    emit result as text
    fail error as text
body
    step lib.FizzBuzz(30 to :n) then
        next :result to output
    done
    emit output to :result
done

test main
    mock lib.FizzBuzz => "done"
    result = main()
    must result == "done"
done
`,
      },
      {
        name: "lib/FizzBuzz.fa",
        source: `docs FizzBuzz
    Prints FizzBuzz for numbers 1 to n.

    docs n
        Upper bound (inclusive).
    done
done

func FizzBuzz
    take n as long
    emit result as text
    fail error as text
body
    nums = list.range(1, n + 1)
    loop nums as i
        by3 = i % 3
        by5 = i % 5
        case
            when by3 == 0
                case
                    when by5 == 0
                        _ = term.print("FizzBuzz")
                    else
                        _ = term.print("Fizz")
                done
            when by5 == 0
                _ = term.print("Buzz")
            else
                label = to.text(i)
                _ = term.print(label)
        done
    done
    emit "done"
done

test FizzBuzz
    mock term.print => true
    result = FizzBuzz(15)
    must result == "done"
done
`,
      },
    ],
  },
  {
    name: "String Processing",
    files: [
      {
        name: "main.fa",
        source: `use lib from "./lib"

docs main
    String analysis pipeline.
done

flow main
    emit result as text
    fail error as text
body
    step lib.Analyze("Hello, World! This is forai." to :text) then
        next :result to report
    done
    emit report to :result
done

test main
    mock lib.Analyze => "ok"
    result = main()
    must result == "ok"
done
`,
      },
      {
        name: "lib/Analyze.fa",
        source: `docs Analyze
    Analyzes a text string and prints statistics.

    docs text
        The text to analyze.
    done
done

func Analyze
    take text as text
    emit result as text
    fail error as text
body
    length = str.len(text)
    upper = str.upper(text)
    lower = str.lower(text)
    words = str.split(text, " ")
    word_count = list.len(words)

    _ = term.print("=== String Analysis ===")
    _ = term.print("Original: #{text}")
    _ = term.print("Length: #{to.text(length)}")
    _ = term.print("Words: #{to.text(word_count)}")
    _ = term.print("Uppercase: #{upper}")
    _ = term.print("Lowercase: #{lower}")

    has_hello = str.contains(text, "Hello")
    case
        when has_hello == true
            _ = term.print("Contains 'Hello': yes")
        else
            _ = term.print("Contains 'Hello': no")
    done

    reversed_words = ""
    indices = list.indices(words)
    loop indices as i
        idx = list.len(words) - 1 - i
        word = words[idx]
        case
            when i == 0
                reversed_words = word
            else
                reversed_words = "#{reversed_words} #{word}"
        done
    done
    _ = term.print("Reversed words: #{reversed_words}")

    emit "ok"
done

test Analyze
    mock term.print => true
    result = Analyze("hello world")
    must result == "ok"
done
`,
      },
    ],
  },
  {
    name: "Math Pipeline",
    files: [
      {
        name: "main.fa",
        source: `use lib from "./lib"

docs main
    Math pipeline: square, double, and format numbers.
done

flow main
    emit result as text
    fail error as text
body
    step lib.MathDemo(7.0 to :num) then
        next :result to output
    done
    emit output to :result
done

test main
    mock lib.MathDemo => "done"
    result = main()
    must result == "done"
done
`,
      },
      {
        name: "lib/MathDemo.fa",
        source: `docs MathDemo
    Demonstrates math operations on a number.

    docs num
        Starting number for the demo.
    done
done

func MathDemo
    take num as real
    emit result as text
    fail error as text
body
    _ = term.print("=== Math Pipeline ===")
    _ = term.print("Input: #{to.text(num)}")

    squared = num * num
    _ = term.print("Squared: #{to.text(squared)}")

    doubled = num * 2.0
    _ = term.print("Doubled: #{to.text(doubled)}")

    cubed = num * num * num
    _ = term.print("Cubed: #{to.text(cubed)}")

    remainder = to.long(num) % 3
    case
        when remainder == 0
            _ = term.print("Divisible by 3: yes")
        else
            _ = term.print("Divisible by 3: no (remainder #{to.text(remainder)})")
    done

    sum = 0.0
    nums = list.range(1, to.long(num) + 1)
    loop nums as i
        sum = sum + to.real(i)
    done
    _ = term.print("Sum 1..#{to.text(to.long(num))}: #{to.text(sum)}")

    emit "done"
done

test MathDemo
    mock term.print => true
    result = MathDemo(5.0)
    must result == "done"
done
`,
      },
    ],
  },
  {
    name: "List Operations",
    files: [
      {
        name: "main.fa",
        source: `use lib from "./lib"

docs main
    Demonstrates list operations.
done

flow main
    emit result as text
    fail error as text
body
    step lib.ListDemo() then
        next :result to output
    done
    emit output to :result
done

test main
    mock lib.ListDemo => "done"
    result = main()
    must result == "done"
done
`,
      },
      {
        name: "lib/ListDemo.fa",
        source: `docs ListDemo
    Demonstrates list creation and manipulation.
done

func ListDemo
    emit result as text
    fail error as text
body
    _ = term.print("=== List Operations ===")

    # Build a list
    items = list.new()
    items = list.append(items, "apple")
    items = list.append(items, "banana")
    items = list.append(items, "cherry")
    items = list.append(items, "date")
    items = list.append(items, "elderberry")

    count = list.len(items)
    _ = term.print("Items: #{to.text(count)}")

    # Print each item
    loop items as item
        _ = term.print("  - #{item}")
    done

    # Check containment
    has_banana = list.contains(items, "banana")
    has_fig = list.contains(items, "fig")
    _ = term.print("Contains banana: #{to.text(has_banana)}")
    _ = term.print("Contains fig: #{to.text(has_fig)}")

    # Slice
    middle = list.slice(items, 1, 4)
    _ = term.print("Slice [1..4]:")
    loop middle as item
        _ = term.print("  - #{item}")
    done

    # First and last
    first = items[0]
    last = items[-1]
    _ = term.print("First: #{first}")
    _ = term.print("Last: #{last}")

    emit "done"
done

test ListDemo
    mock term.print => true
    result = ListDemo()
    must result == "done"
done
`,
      },
    ],
  },
];

// --- State ---

interface FileState {
  name: string;
  source: string;
}

let files: FileState[] = [...EXAMPLES[0].files];

let activeFileIndex = 0;
let compileErrors: CompileError[] = [];
let lastIrData: IrData | null = null;
let compilerReady = false;
let compileTimer: ReturnType<typeof setTimeout> | null = null;
let editorView: EditorView | null = null;

// --- Compiler Worker ---

let compilerWorker = new Worker("./compiler-worker.js", { type: "module" });
let compileId = 0;

function handleWorkerMessage(e: MessageEvent<WorkerResponse>) {
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

  if (msg.type === "execute-result") {
    if (msg.id !== executeId) return;
    isRunning = false;
    clearExecuteTimeout();
    const elapsed = msg.elapsed ?? 0;
    if (msg.error) {
      pushOutputLine(`Error: ${msg.error}`, "log-error");
    } else if (msg.result) {
      const r = msg.result;
      if (r.compile_errors && r.compile_errors.length > 0) {
        pushOutputLine("Compilation failed:", "log-error");
        for (const e of r.compile_errors) {
          pushOutputLine(`  ${e.file}:${e.line}:${e.col} ${e.message}`, "log-error");
        }
      } else if (r.error) {
        // Runtime error — show any prints that happened before the error
        if (r.prints) {
          for (const p of r.prints) {
            pushOutputLine(p, "log-line");
          }
        }
        if (r.logs) {
          for (const l of r.logs) {
            pushOutputLine(`[${l.level}] ${l.message}`, logClass(l.level));
          }
        }
        pushOutputLine(`Runtime error: ${r.error}`, "log-error");
      } else if (r.ok) {
        for (const p of r.ok.prints) {
          pushOutputLine(p, "log-line");
        }
        for (const l of r.ok.logs) {
          pushOutputLine(`[${l.level}] ${l.message}`, logClass(l.level));
        }
        if (r.ok.prints.length === 0 && r.ok.logs.length === 0) {
          pushOutputLine("(No output)", "log-debug");
        }
      }
    }
    pushOutputLine(`Executed in ${elapsed.toFixed(0)}ms`, "log-debug");
    activeOutputTab = "output";
    document.querySelectorAll(".output-tab").forEach((t) => t.classList.remove("active"));
    document.querySelector('.output-tab[data-panel="output"]')?.classList.add("active");
    updateOutputPanel();
    setStatus("Ready");
  }

  if (msg.type === "debug-result") {
    if (msg.id !== executeId) return;
    isRunning = false;
    clearExecuteTimeout();
    if (msg.error) {
      pushOutputLine(`Debug error: ${msg.error}`, "log-error");
      setStatus("Ready");
    } else if (msg.result) {
      const r = msg.result;
      if (r.compile_errors && r.compile_errors.length > 0) {
        pushOutputLine("Compilation failed:", "log-error");
        for (const e of r.compile_errors) {
          pushOutputLine(`  ${e.file}:${e.line}:${e.col} ${e.message}`, "log-error");
        }
        setStatus("Ready");
      } else if (r.ok) {
        debugSnapshots = r.ok.snapshots;
        debugStepIndex = -1;
        isDebugMode = true;
        // Show prints from execution
        for (const p of r.ok.prints) {
          pushOutputLine(p, "log-line");
        }
        for (const l of r.ok.logs) {
          pushOutputLine(`[${l.level}] ${l.message}`, logClass(l.level));
        }
        pushOutputLine(`Debug: ${debugSnapshots.length} step(s) captured`, "log-debug");
        showDebugToolbar();
        setStatus(`Debug: ${debugSnapshots.length} steps — use Step/Continue`);
      } else if (r.error) {
        pushOutputLine(`Runtime error: ${r.error}`, "log-error");
        setStatus("Ready");
      }
    }
    activeOutputTab = "output";
    document.querySelectorAll(".output-tab").forEach((t) => t.classList.remove("active"));
    document.querySelector('.output-tab[data-panel="output"]')?.classList.add("active");
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
}

compilerWorker.onmessage = handleWorkerMessage;

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
          scheduleAutoSave();
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
let outputLines: { text: string; cls: string }[] = [];
let executeId = 0;
let isRunning = false;

// Debug mode state
let debugSnapshots: StepSnapshot[] = [];
let debugStepIndex = -1;
let debugBreakpoints = new Set<string>();
let isDebugMode = false;

function updateOutputPanel() {
  const panel = document.getElementById("output-panel")!;

  if (activeOutputTab === "output") {
    if (outputLines.length === 0) {
      panel.innerHTML = '<div class="empty-state">Press Run to execute your code</div>';
    } else {
      panel.innerHTML = outputLines
        .map((l) => `<div class="${l.cls}">${escapeHtml(l.text)}</div>`)
        .join("");
    }
  } else if (activeOutputTab === "ir-graph") {
    if (lastIrData) {
      panel.innerHTML = "";
      renderGraph(lastIrData, panel);
    } else {
      panel.innerHTML = '<div class="empty-state">No IR available</div>';
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
  if (!compilerReady || isRunning) return;
  isRunning = true;
  outputLines = [];
  setStatus("Running...");

  const fileMap: Record<string, string> = {};
  for (const f of files) {
    fileMap[f.name] = f.source;
  }

  executeId++;
  compilerWorker.postMessage({
    type: "execute",
    id: executeId,
    files: fileMap,
    entryPoint: "main.fa",
  });

  startExecuteTimeout();

  // Show "Running..." in output while waiting
  activeOutputTab = "output";
  document.querySelectorAll(".output-tab").forEach((t) => t.classList.remove("active"));
  document.querySelector('.output-tab[data-panel="output"]')?.classList.add("active");
  updateOutputPanel();
}

// --- Debug ---

function handleDebug() {
  if (!compilerReady || isRunning) return;
  isRunning = true;
  outputLines = [];
  debugSnapshots = [];
  debugStepIndex = -1;
  isDebugMode = false;
  setStatus("Debugging...");

  const fileMap: Record<string, string> = {};
  for (const f of files) {
    fileMap[f.name] = f.source;
  }

  executeId++;
  compilerWorker.postMessage({
    type: "debug",
    id: executeId,
    files: fileMap,
    entryPoint: "main.fa",
  });

  startExecuteTimeout();
}

function showDebugToolbar() {
  let toolbar = document.getElementById("debug-toolbar");
  if (!toolbar) {
    toolbar = document.createElement("div");
    toolbar.id = "debug-toolbar";
    toolbar.style.cssText = "display:flex;align-items:center;gap:6px;padding:4px 12px;background:var(--bg-tertiary);border-bottom:1px solid var(--border);font-size:12px;";
    const outputTabs = document.querySelector(".output-tabs")!;
    outputTabs.parentElement!.insertBefore(toolbar, outputTabs.nextSibling);
  }
  toolbar.innerHTML = "";
  toolbar.style.display = "flex";

  const btnStep = document.createElement("button");
  btnStep.textContent = "Step";
  btnStep.style.cssText = "background:var(--bg);border:1px solid var(--border);color:var(--text);padding:2px 10px;border-radius:4px;cursor:pointer;font-size:11px;";
  btnStep.onclick = debugStep;

  const btnContinue = document.createElement("button");
  btnContinue.textContent = "Continue";
  btnContinue.style.cssText = btnStep.style.cssText;
  btnContinue.onclick = debugContinue;

  const btnRun = document.createElement("button");
  btnRun.textContent = "Run All";
  btnRun.style.cssText = btnStep.style.cssText;
  btnRun.onclick = debugRunAll;

  const btnStop = document.createElement("button");
  btnStop.textContent = "Stop";
  btnStop.style.cssText = btnStep.style.cssText;
  btnStop.onclick = debugStop;

  const info = document.createElement("span");
  info.id = "debug-info";
  info.style.cssText = "color:var(--text-muted);margin-left:auto;";
  info.textContent = `Step 0 / ${debugSnapshots.length}`;

  toolbar.appendChild(btnStep);
  toolbar.appendChild(btnContinue);
  toolbar.appendChild(btnRun);
  toolbar.appendChild(btnStop);
  toolbar.appendChild(info);
}

function hideDebugToolbar() {
  const toolbar = document.getElementById("debug-toolbar");
  if (toolbar) toolbar.style.display = "none";
  isDebugMode = false;
  debugSnapshots = [];
  debugStepIndex = -1;
}

function debugStep() {
  if (debugStepIndex < debugSnapshots.length - 1) {
    debugStepIndex++;
    renderDebugState();
  }
}

function debugContinue() {
  // Step until next breakpoint or end
  while (debugStepIndex < debugSnapshots.length - 1) {
    debugStepIndex++;
    const snap = debugSnapshots[debugStepIndex];
    const nodeId = `n${snap.step}_${snap.bind}`;
    if (debugBreakpoints.has(nodeId) || debugBreakpoints.has(snap.bind)) {
      break;
    }
  }
  renderDebugState();
}

function debugRunAll() {
  debugStepIndex = debugSnapshots.length - 1;
  renderDebugState();
}

function debugStop() {
  hideDebugToolbar();
  setStatus("Ready");
  updateOutputPanel();
}

function renderDebugState() {
  if (debugStepIndex < 0 || debugStepIndex >= debugSnapshots.length) return;
  const snap = debugSnapshots[debugStepIndex];

  // Update info label
  const info = document.getElementById("debug-info");
  if (info) info.textContent = `Step ${debugStepIndex + 1} / ${debugSnapshots.length} — ${snap.op} → ${snap.bind}`;
  setStatus(`Debug step ${debugStepIndex + 1}: ${snap.op}`);

  // Update graph if visible
  if (activeOutputTab === "ir-graph" && lastIrData) {
    const graphState: GraphState = {
      currentNodeId: null, // We don't have IR node IDs in sync_runtime snapshots
      executedNodes: new Set(debugSnapshots.slice(0, debugStepIndex + 1).map((s) => s.bind)),
      failedNodes: new Set<string>(),
      iterationExecuted: new Set(debugSnapshots.slice(0, debugStepIndex + 1).map((s) => s.bind)),
      breakpoints: debugBreakpoints,
      bindings: snap.bindings,
    };
    const panel = document.getElementById("output-panel")!;
    renderGraph(lastIrData, panel, graphState);
  }

  // Update variables panel inline in output if on output tab
  if (activeOutputTab === "output") {
    renderVariablesPanel(snap);
  }
}

function renderVariablesPanel(snap: StepSnapshot) {
  const panel = document.getElementById("output-panel")!;
  let html = '<div style="padding:4px 0;border-bottom:1px solid var(--border);margin-bottom:8px;">';
  html += `<span style="color:var(--accent);font-weight:600;">Step ${snap.step + 1}</span>`;
  html += ` <span style="color:var(--text-muted);">${escapeHtml(snap.op)} → ${escapeHtml(snap.bind)}</span></div>`;

  // Variables
  const entries = Object.entries(snap.bindings);
  if (entries.length > 0) {
    html += '<div style="font-size:12px;">';
    for (const [k, v] of entries) {
      if (k.startsWith("_step_")) continue;
      const val = typeof v === "object" ? JSON.stringify(v) : String(v);
      html += `<div style="display:flex;gap:8px;padding:2px 0;border-bottom:1px solid var(--border);">`;
      html += `<span style="color:var(--accent);min-width:80px;">${escapeHtml(k)}</span>`;
      html += `<span style="color:var(--text);word-break:break-all;">${escapeHtml(val)}</span></div>`;
    }
    html += "</div>";
  }

  // Show output lines below
  if (outputLines.length > 0) {
    html += '<div style="margin-top:8px;padding-top:8px;border-top:1px solid var(--border);">';
    html += outputLines.map((l) => `<div class="${l.cls}">${escapeHtml(l.text)}</div>`).join("");
    html += "</div>";
  }

  panel.innerHTML = html;
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

function logClass(level: string): string {
  switch (level) {
    case "error": return "log-error";
    case "warn": return "log-warn";
    case "debug": case "trace": return "log-debug";
    default: return "log-info";
  }
}

function escapeHtml(s: string): string {
  return s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
}

// --- Auto-save ---

let autoSaveTimer: ReturnType<typeof setTimeout> | null = null;

function scheduleAutoSave() {
  if (autoSaveTimer) clearTimeout(autoSaveTimer);
  autoSaveTimer = setTimeout(() => {
    try {
      const state = JSON.stringify({ files, activeFileIndex });
      localStorage.setItem(AUTOSAVE_KEY, state);
    } catch {
      // localStorage full or unavailable — ignore
    }
  }, AUTOSAVE_DEBOUNCE_MS);
}

function restoreAutoSave(): boolean {
  try {
    const raw = localStorage.getItem(AUTOSAVE_KEY);
    if (!raw) return false;
    const state = JSON.parse(raw);
    if (state.files && Array.isArray(state.files) && state.files.length > 0) {
      files = state.files;
      activeFileIndex = typeof state.activeFileIndex === "number" ? state.activeFileIndex : 0;
      if (activeFileIndex >= files.length) activeFileIndex = 0;
      return true;
    }
  } catch {
    // Corrupted state — ignore
  }
  return false;
}

// --- URL Sharing ---

function encodeShareUrl(): string {
  const state = JSON.stringify({ files, activeFileIndex });
  const encoded = btoa(unescape(encodeURIComponent(state)));
  return `${location.origin}${location.pathname}#code=${encoded}`;
}

function decodeShareUrl(): boolean {
  const hash = location.hash;
  if (!hash.startsWith("#code=")) return false;
  try {
    const encoded = hash.slice(6);
    const json = decodeURIComponent(escape(atob(encoded)));
    const state = JSON.parse(json);
    if (state.files && Array.isArray(state.files) && state.files.length > 0) {
      files = state.files;
      activeFileIndex = typeof state.activeFileIndex === "number" ? state.activeFileIndex : 0;
      if (activeFileIndex >= files.length) activeFileIndex = 0;
      return true;
    }
  } catch {
    // Invalid share URL — ignore
  }
  return false;
}

function handleShare() {
  const url = encodeShareUrl();
  navigator.clipboard.writeText(url).then(() => {
    showToast("Link copied to clipboard");
  }, () => {
    // Fallback: show in prompt
    prompt("Share URL:", url);
  });
}

// --- Toast ---

function showToast(message: string) {
  let toast = document.querySelector(".toast") as HTMLElement | null;
  if (!toast) {
    toast = document.createElement("div");
    toast.className = "toast";
    document.body.appendChild(toast);
  }
  toast.textContent = message;
  toast.classList.add("visible");
  setTimeout(() => toast!.classList.remove("visible"), 2500);
}

// --- Examples ---

function initExamples() {
  const select = document.getElementById("examples-select") as HTMLSelectElement;
  for (const ex of EXAMPLES) {
    const opt = document.createElement("option");
    opt.value = ex.name;
    opt.textContent = ex.name;
    select.appendChild(opt);
  }
  select.onchange = () => {
    const name = select.value;
    if (!name) return;
    const example = EXAMPLES.find((e) => e.name === name);
    if (example) {
      loadFiles(example.files);
      showToast(`Loaded: ${example.name}`);
    }
    select.value = "";
  };
}

function loadFiles(newFiles: FileState[]) {
  files = newFiles.map((f) => ({ ...f }));
  activeFileIndex = 0;
  if (editorView) {
    editorView.setState(createEditorState(files[0].source));
  }
  renderTabs();
  triggerCompile();
  outputLines = [];
  updateOutputPanel();
  scheduleAutoSave();
  document.getElementById("file-count")!.textContent = `${files.length} file(s)`;
}

// --- Execution Timeout ---

let executeTimeoutTimer: ReturnType<typeof setTimeout> | null = null;

function startExecuteTimeout() {
  clearExecuteTimeout();
  executeTimeoutTimer = setTimeout(() => {
    if (isRunning) {
      isRunning = false;
      outputLines.push({ text: `Execution timed out after ${EXECUTE_TIMEOUT_MS / 1000}s`, cls: "log-error" });
      activeOutputTab = "output";
      updateOutputPanel();
      setStatus("Timed out");
      // Terminate and restart worker
      compilerWorker.terminate();
      restartWorker();
    }
  }, EXECUTE_TIMEOUT_MS);
}

function clearExecuteTimeout() {
  if (executeTimeoutTimer) {
    clearTimeout(executeTimeoutTimer);
    executeTimeoutTimer = null;
  }
}

function restartWorker() {
  compilerReady = false;
  setStatus("Restarting compiler...");
  compilerWorker = new Worker("./compiler-worker.js", { type: "module" });
  compilerWorker.onmessage = handleWorkerMessage;
}

// --- Output Buffer ---

function pushOutputLine(text: string, cls: string) {
  if (outputLines.length >= OUTPUT_MAX_LINES) {
    if (outputLines.length === OUTPUT_MAX_LINES) {
      outputLines.push({ text: "[Output truncated — limit reached]", cls: "log-warn" });
    }
    return;
  }
  outputLines.push({ text, cls });
}

// --- Clear ---

function handleClear() {
  outputLines = [];
  updateOutputPanel();
}

// --- Init ---

function init() {
  // Restore state: URL hash takes priority, then localStorage
  let restored = false;
  if (location.hash.startsWith("#code=")) {
    restored = decodeShareUrl();
    if (restored) showToast("Loaded from shared link");
  }
  if (!restored) {
    restored = restoreAutoSave();
    if (restored) showToast("Restored from auto-save");
  }

  initEditor();
  renderTabs();
  initOutputTabs();
  initResize();
  initExamples();

  document.getElementById("btn-run")!.onclick = handleRun;
  document.getElementById("btn-debug")!.onclick = handleDebug;
  document.getElementById("btn-format")!.onclick = handleFormat;
  document.getElementById("btn-share")!.onclick = handleShare;
  document.getElementById("btn-clear")!.onclick = handleClear;

  // Detect platform for keyboard shortcut display
  const isMac = /Mac|iPod|iPhone|iPad/.test(navigator.platform);
  document.getElementById("run-shortcut")!.textContent = isMac ? "Cmd+Enter" : "Ctrl+Enter";

  // Update file count
  document.getElementById("file-count")!.textContent = `${files.length} file(s)`;
}

init();
