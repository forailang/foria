class FakeStyle {
  setProperty(key, value) {
    this[key] = String(value);
  }

  removeProperty(key) {
    delete this[key];
  }
}

class FakeNode {
  constructor() {
    this.parentNode = null;
    this.childNodes = [];
  }

  appendChild(node) {
    node.parentNode = this;
    this.childNodes.push(node);
    return node;
  }

  removeChild(node) {
    const idx = this.childNodes.indexOf(node);
    if (idx >= 0) {
      this.childNodes.splice(idx, 1);
      node.parentNode = null;
    }
    return node;
  }

  replaceChild(next, prev) {
    const idx = this.childNodes.indexOf(prev);
    if (idx >= 0) {
      this.childNodes[idx] = next;
      next.parentNode = this;
      prev.parentNode = null;
    }
    return prev;
  }
}

class FakeElement extends FakeNode {
  constructor(tagName) {
    super();
    this.tagName = tagName.toUpperCase();
    this.style = new FakeStyle();
    this.className = "";
    this.textContent = "";
    this.id = "";
    this._listeners = new Map();
  }

  addEventListener(type, fn) {
    const list = this._listeners.get(type) ?? [];
    list.push(fn);
    this._listeners.set(type, list);
  }

  removeEventListener(type, fn) {
    const list = this._listeners.get(type) ?? [];
    const idx = list.indexOf(fn);
    if (idx >= 0) list.splice(idx, 1);
    this._listeners.set(type, list);
  }

  dispatchEvent(event) {
    const list = this._listeners.get(event.type) ?? [];
    for (const fn of list) fn(event);
    return true;
  }

  click() {
    this.dispatchEvent({ type: "click" });
  }
}

class FakeInputElement extends FakeElement {
  constructor() {
    super("input");
    this.type = "text";
    this.placeholder = "";
    this.value = "";
    this.checked = false;
  }
}

class FakeDocument {
  constructor() {
    this._roots = new Map();
    this.title = "";
  }

  createElement(tag) {
    if (tag === "input") return new FakeInputElement();
    return new FakeElement(tag);
  }

  querySelector(selector) {
    if (!selector.startsWith("#")) return null;
    return this._roots.get(selector.slice(1)) ?? null;
  }

  createRoot(id = "app") {
    const root = new FakeElement("div");
    root.id = id;
    this._roots.set(id, root);
    return root;
  }
}

export function installFakeDom() {
  const doc = new FakeDocument();
  const app = doc.createRoot("app");

  const history = {
    _path: "/",
    pushState(_state, _title, path) {
      this._path = String(path);
    },
  };

  const windowObj = {
    location: { pathname: "/" },
    _listeners: new Map(),
    addEventListener(type, fn) {
      const list = this._listeners.get(type) ?? [];
      list.push(fn);
      this._listeners.set(type, list);
    },
    removeEventListener(type, fn) {
      const list = this._listeners.get(type) ?? [];
      const idx = list.indexOf(fn);
      if (idx >= 0) list.splice(idx, 1);
      this._listeners.set(type, list);
    },
    emit(type) {
      const list = this._listeners.get(type) ?? [];
      for (const fn of list) fn();
    },
  };

  globalThis.document = doc;
  globalThis.window = windowObj;
  globalThis.history = history;
  globalThis.HTMLElement = FakeElement;
  globalThis.HTMLInputElement = FakeInputElement;

  return {
    document: doc,
    app,
    window: windowObj,
    history,
    teardown() {
      delete globalThis.document;
      delete globalThis.window;
      delete globalThis.history;
      delete globalThis.HTMLElement;
      delete globalThis.HTMLInputElement;
    },
  };
}
