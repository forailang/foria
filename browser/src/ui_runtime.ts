type UiProps = Record<string, unknown>;
type UiNode = {
  type?: unknown;
  props?: unknown;
  children?: unknown;
  events?: unknown;
};

const UI_RESERVED_PROPS = new Set([
  "value", "label", "placeholder", "spacing",
  "padding", "margin", "align", "width", "height",
  "color", "bg", "border", "size", "bold", "italic",
]);

function asObj(v: unknown): Record<string, unknown> {
  return typeof v === "object" && v !== null ? (v as Record<string, unknown>) : {};
}

function asNode(v: unknown): UiNode {
  const obj = asObj(v);
  return {
    type: obj.type,
    props: obj.props,
    children: obj.children,
    events: obj.events,
  };
}

function cssLen(v: unknown): string | null {
  if (typeof v === "number") return `${v}px`;
  if (typeof v === "string") return v;
  return null;
}

function alignToCss(v: unknown): string {
  if (v === "start") return "flex-start";
  if (v === "center") return "center";
  if (v === "end") return "flex-end";
  return "";
}

function tagForType(nodeType: string): string {
  switch (nodeType) {
    case "screen":
    case "vstack":
    case "hstack":
    case "zstack":
      return "div";
    case "text":
      return "span";
    case "button":
      return "button";
    case "input":
    case "toggle":
      return "input";
    default:
      return "div";
  }
}

function applyTextContent(el: HTMLElement, nodeType: string, props: UiProps): void {
  if (nodeType === "text") {
    el.textContent = typeof props.value === "string" ? props.value : "";
  } else if (nodeType === "button") {
    el.textContent = typeof props.label === "string" ? props.label : "";
  }
}

function applyInputState(el: HTMLElement, nodeType: string, props: UiProps): void {
  if (!(el instanceof HTMLInputElement)) return;
  if (nodeType === "input") {
    el.type = "text";
    el.placeholder = typeof props.placeholder === "string" ? props.placeholder : "";
    el.value = typeof props.value === "string" ? props.value : "";
  } else if (nodeType === "toggle") {
    el.type = "checkbox";
    el.checked = props.value === true;
  }
}

function applyCommonStyles(style: CSSStyleDeclaration, props: UiProps): void {
  style.padding = "";
  style.margin = "";
  style.width = "";
  style.height = "";
  style.color = "";
  style.background = "";
  style.border = "";
  style.fontWeight = "";
  style.fontStyle = "";
  style.fontSize = "";

  const padding = cssLen(props.padding);
  if (padding) style.padding = padding;
  const margin = cssLen(props.margin);
  if (margin) style.margin = margin;
  const width = cssLen(props.width);
  if (width) style.width = width;
  const height = cssLen(props.height);
  if (height) style.height = height;
  if (typeof props.color === "string") style.color = props.color;
  if (typeof props.bg === "string") style.background = props.bg;
  if (props.border === true) style.border = "1px solid currentColor";
  if (typeof props.border === "string") style.border = props.border;
  if (props.bold === true) style.fontWeight = "bold";
  if (props.italic === true) style.fontStyle = "italic";
  const size = cssLen(props.size);
  if (size) style.fontSize = size;

  for (const [key, value] of Object.entries(props)) {
    if (UI_RESERVED_PROPS.has(key)) continue;
    if (typeof value === "string" || typeof value === "number") {
      style.setProperty(key, String(value));
    } else {
      style.removeProperty(key);
    }
  }
}

function applyLayoutStyles(style: CSSStyleDeclaration, nodeType: string, props: UiProps): void {
  style.display = "";
  style.flexDirection = "";
  style.gap = "";
  style.alignItems = "";
  style.position = "";

  if (nodeType === "vstack" || nodeType === "hstack") {
    style.display = "flex";
    style.flexDirection = nodeType === "vstack" ? "column" : "row";
    if (typeof props.spacing === "number") {
      style.gap = `${props.spacing}px`;
    }
    const align = alignToCss(props.align);
    if (align) style.alignItems = align;
  }
  if (nodeType === "zstack") {
    style.position = "relative";
  }
}

type Cleanup = () => void;

export type UiDebugEvent = {
  kind: "tree" | "event" | "nav" | "lifecycle";
  action?: string;
  selector?: string;
  path?: string;
  event?: unknown;
  tree?: unknown;
};

export type UiRuntime = {
  mount: (tree: unknown, selector?: string) => boolean;
  update: (tree: unknown) => boolean;
  navigate: (path: string) => boolean;
  unmount: () => void;
};

export function createUiRuntime(
  enqueueUiEvent: (event: unknown) => void,
  onDebug?: (event: UiDebugEvent) => void,
): UiRuntime {
  const listenerCleanup = new WeakMap<HTMLElement, Cleanup[]>();
  let rootContainer: Element | null = null;
  let rootSelector = "#app";
  let rootDom: Node | null = null;
  let currentTree: UiNode | null = null;

  function clearListeners(el: HTMLElement): void {
    const cleanups = listenerCleanup.get(el) ?? [];
    for (const cleanup of cleanups) cleanup();
    listenerCleanup.delete(el);
  }

  function cleanupSubtree(node: Node): void {
    if (node instanceof HTMLElement) {
      clearListeners(node);
      for (const child of Array.from(node.childNodes)) {
        cleanupSubtree(child);
      }
    }
  }

  function wireEvents(el: HTMLElement, nodeType: string, events: Record<string, unknown>): void {
    clearListeners(el);
    const cleanups: Cleanup[] = [];

    for (const [action, eventValue] of Object.entries(events)) {
      if (nodeType === "input") {
        const handler = () => {
          const input = el as HTMLInputElement;
          const event = { type: "input", action, value: input.value };
          enqueueUiEvent(event);
          onDebug?.({ kind: "event", action: "enqueue", event });
        };
        el.addEventListener("input", handler);
        cleanups.push(() => el.removeEventListener("input", handler));
        continue;
      }

      if (nodeType === "toggle") {
        const handler = () => {
          const input = el as HTMLInputElement;
          const event = { type: "toggle", action, value: input.checked };
          enqueueUiEvent(event);
          onDebug?.({ kind: "event", action: "enqueue", event });
        };
        el.addEventListener("change", handler);
        cleanups.push(() => el.removeEventListener("change", handler));
        continue;
      }

      const handler = () => {
        const event = { type: "action", action, value: eventValue };
        enqueueUiEvent(event);
        onDebug?.({ kind: "event", action: "enqueue", event });
      };
      el.addEventListener("click", handler);
      cleanups.push(() => el.removeEventListener("click", handler));
    }

    listenerCleanup.set(el, cleanups);
  }

  function applyElement(el: HTMLElement, node: UiNode): void {
    const nodeType = typeof node.type === "string" ? node.type : "div";
    const props = asObj(node.props);
    const events = asObj(node.events);

    if (nodeType === "screen") {
      el.className = "forai-screen";
    } else if (el.className === "forai-screen") {
      el.className = "";
    }

    applyTextContent(el, nodeType, props);
    applyInputState(el, nodeType, props);
    applyLayoutStyles(el.style, nodeType, props);
    applyCommonStyles(el.style, props);
    wireEvents(el, nodeType, events);
  }

  function createDom(nodeLike: unknown): Node {
    const node = asNode(nodeLike);
    const nodeType = typeof node.type === "string" ? node.type : "div";
    const tag = tagForType(nodeType);
    const el = document.createElement(tag);
    applyElement(el, node);
    const children = Array.isArray(node.children) ? node.children : [];
    for (const child of children) {
      el.appendChild(createDom(child));
    }
    return el;
  }

  function patchDom(existing: Node, prevLike: unknown, nextLike: unknown): Node {
    const prev = asNode(prevLike);
    const next = asNode(nextLike);
    const prevType = typeof prev.type === "string" ? prev.type : "div";
    const nextType = typeof next.type === "string" ? next.type : "div";
    const prevTag = tagForType(prevType);
    const nextTag = tagForType(nextType);

    if (!(existing instanceof HTMLElement) || prevTag !== nextTag) {
      const replacement = createDom(next);
      cleanupSubtree(existing);
      return replacement;
    }

    const el = existing;
    applyElement(el, next);

    const prevChildren = Array.isArray(prev.children) ? prev.children : [];
    const nextChildren = Array.isArray(next.children) ? next.children : [];
    const maxLen = Math.max(prevChildren.length, nextChildren.length);

    for (let i = 0; i < maxLen; i += 1) {
      const prevChild = prevChildren[i];
      const nextChild = nextChildren[i];
      const domChild = el.childNodes[i];

      if (nextChild === undefined) {
        if (domChild) {
          cleanupSubtree(domChild);
          el.removeChild(domChild);
        }
        continue;
      }

      if (prevChild === undefined || !domChild) {
        el.appendChild(createDom(nextChild));
        continue;
      }

      const patchedChild = patchDom(domChild, prevChild, nextChild);
      if (patchedChild !== domChild) {
        el.replaceChild(patchedChild, domChild);
      }
    }

    return el;
  }

  function mount(tree: unknown, selector = "#app"): boolean {
    if (typeof document === "undefined") return true;
    const container = document.querySelector(selector);
    if (!container) {
      throw new Error(`ui.mount: selector not found: ${selector}`);
    }

    rootContainer = container;
    rootSelector = selector;
    currentTree = asNode(tree);
    container.innerHTML = "";
    rootDom = createDom(currentTree);
    container.appendChild(rootDom);
    onDebug?.({ kind: "tree", action: "mount", selector, tree: currentTree });
    return true;
  }

  function update(tree: unknown): boolean {
    if (typeof document === "undefined") return true;
    const nextTree = asNode(tree);
    if (!rootContainer || !rootDom || !currentTree) {
      return mount(nextTree, rootSelector);
    }

    const patchedRoot = patchDom(rootDom, currentTree, nextTree);
    if (patchedRoot !== rootDom) {
      rootContainer.replaceChild(patchedRoot, rootDom);
    }
    rootDom = patchedRoot;
    currentTree = nextTree;
    onDebug?.({ kind: "tree", action: "update", tree: nextTree });
    return true;
  }

  function navigate(path: string): boolean {
    if (typeof window !== "undefined" && typeof history !== "undefined") {
      history.pushState({}, "", path);
      enqueueUiEvent({ type: "nav", path });
      onDebug?.({ kind: "nav", action: "navigate", path });
    }
    return true;
  }

  function unmount(): void {
    if (rootDom) cleanupSubtree(rootDom);
    onDebug?.({ kind: "lifecycle", action: "unmount" });
    rootDom = null;
    currentTree = null;
    rootContainer = null;
  }

  return {
    mount,
    update,
    navigate,
    unmount,
  };
}
