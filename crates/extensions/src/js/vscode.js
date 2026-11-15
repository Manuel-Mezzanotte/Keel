// The `vscode` module as Keel's extension host provides it.
//
// Everything that only needs data lives here; anything that touches the
// editor or the window is sent to Keel through `host.call` (waits for an
// answer) or `host.notify` (does not). The API grows piece by piece, starting
// with what VSAuto Ghost uses.
"use strict";

((globalThis) => {
  const { host } = globalThis.__keel;

  // ---------------------------------------------------------------------
  // Basic types

  class Disposable {
    constructor(callOnDispose) {
      this._callOnDispose = callOnDispose;
    }
    static from(...disposables) {
      return new Disposable(() => {
        for (const disposable of disposables) disposable?.dispose?.();
      });
    }
    dispose() {
      const callback = this._callOnDispose;
      this._callOnDispose = undefined;
      callback?.();
    }
  }

  class EventEmitter {
    constructor() {
      this._listeners = new Set();
      this.event = (listener, thisArgs, disposables) => {
        const entry = { listener, thisArgs };
        this._listeners.add(entry);
        const disposable = new Disposable(() => this._listeners.delete(entry));
        if (Array.isArray(disposables)) disposables.push(disposable);
        return disposable;
      };
    }
    fire(data) {
      for (const { listener, thisArgs } of [...this._listeners]) {
        try {
          listener.call(thisArgs, data);
        } catch (error) {
          console.error("an event listener failed:", error);
        }
      }
    }
    dispose() {
      this._listeners.clear();
    }
  }

  class CancellationTokenSource {
    constructor() {
      const emitter = new EventEmitter();
      this._emitter = emitter;
      this.token = { isCancellationRequested: false, onCancellationRequested: emitter.event };
    }
    cancel() {
      if (!this.token.isCancellationRequested) {
        this.token.isCancellationRequested = true;
        this._emitter.fire();
      }
    }
    dispose() {
      this._emitter.dispose();
    }
  }
  const noCancellation = new CancellationTokenSource().token;

  class Position {
    constructor(line, character) {
      if (line < 0 || character < 0) throw new Error("line and character must not be negative");
      this.line = line;
      this.character = character;
    }
    compareTo(other) {
      return this.line - other.line || this.character - other.character;
    }
    isBefore(other) {
      return this.compareTo(other) < 0;
    }
    isBeforeOrEqual(other) {
      return this.compareTo(other) <= 0;
    }
    isAfter(other) {
      return this.compareTo(other) > 0;
    }
    isAfterOrEqual(other) {
      return this.compareTo(other) >= 0;
    }
    isEqual(other) {
      return this.compareTo(other) === 0;
    }
    translate(lineDeltaOrChange = 0, characterDelta = 0) {
      if (typeof lineDeltaOrChange === "object") {
        return new Position(
          this.line + (lineDeltaOrChange.lineDelta ?? 0),
          this.character + (lineDeltaOrChange.characterDelta ?? 0),
        );
      }
      return new Position(this.line + lineDeltaOrChange, this.character + characterDelta);
    }
    with(lineOrChange, character) {
      if (typeof lineOrChange === "object") {
        return new Position(lineOrChange.line ?? this.line, lineOrChange.character ?? this.character);
      }
      return new Position(lineOrChange ?? this.line, character ?? this.character);
    }
  }

  function toPosition(value) {
    return value instanceof Position ? value : new Position(value.line, value.character);
  }

  class Range {
    constructor(startLine, startCharacter, endLine, endCharacter) {
      let start;
      let end;
      if (typeof startLine === "number") {
        start = new Position(startLine, startCharacter);
        end = new Position(endLine, endCharacter);
      } else {
        start = toPosition(startLine);
        end = toPosition(startCharacter);
      }
      if (start.isAfter(end)) [start, end] = [end, start];
      this.start = start;
      this.end = end;
    }
    get isEmpty() {
      return this.start.isEqual(this.end);
    }
    get isSingleLine() {
      return this.start.line === this.end.line;
    }
    contains(value) {
      if (value instanceof Range) return this.contains(value.start) && this.contains(value.end);
      const position = toPosition(value);
      return position.isAfterOrEqual(this.start) && position.isBeforeOrEqual(this.end);
    }
    isEqual(other) {
      return this.start.isEqual(other.start) && this.end.isEqual(other.end);
    }
    intersection(other) {
      const start = this.start.isAfter(other.start) ? this.start : other.start;
      const end = this.end.isBefore(other.end) ? this.end : other.end;
      return start.isAfter(end) ? undefined : new Range(start, end);
    }
    union(other) {
      const start = this.start.isBefore(other.start) ? this.start : other.start;
      const end = this.end.isAfter(other.end) ? this.end : other.end;
      return new Range(start, end);
    }
    with(startOrChange, end) {
      if (startOrChange && !(startOrChange instanceof Position) && typeof startOrChange === "object") {
        return new Range(startOrChange.start ?? this.start, startOrChange.end ?? this.end);
      }
      return new Range(startOrChange ?? this.start, end ?? this.end);
    }
  }

  class Selection extends Range {
    constructor(anchorLine, anchorCharacter, activeLine, activeCharacter) {
      let anchor;
      let active;
      if (typeof anchorLine === "number") {
        anchor = new Position(anchorLine, anchorCharacter);
        active = new Position(activeLine, activeCharacter);
      } else {
        anchor = toPosition(anchorLine);
        active = toPosition(anchorCharacter);
      }
      super(anchor, active);
      this.anchor = anchor;
      this.active = active;
    }
    get isReversed() {
      return this.anchor.isAfter(this.active);
    }
  }

  class Uri {
    constructor(scheme, authority, path, query, fragment) {
      this.scheme = scheme;
      this.authority = authority ?? "";
      this.path = path ?? "";
      this.query = query ?? "";
      this.fragment = fragment ?? "";
    }
    static file(fsPath) {
      return new Uri("file", "", fsPath.startsWith("/") ? fsPath : "/" + fsPath, "", "");
    }
    static parse(value) {
      const match = /^([a-zA-Z][\w+.-]*):(?:\/\/([^/?#]*))?([^?#]*)(?:\?([^#]*))?(?:#(.*))?$/.exec(value);
      if (!match) throw new Error(`not a valid URI: ${value}`);
      const decode = (part) => {
        try {
          return decodeURIComponent(part ?? "");
        } catch {
          return part ?? "";
        }
      };
      return new Uri(match[1], decode(match[2]), decode(match[3]), decode(match[4]), decode(match[5]));
    }
    static from(components) {
      return new Uri(components.scheme, components.authority, components.path, components.query, components.fragment);
    }
    static joinPath(base, ...segments) {
      const path = globalThis.__keel.builtin("path");
      return base.with({ path: path.join(base.path || "/", ...segments) });
    }
    get fsPath() {
      return this.path;
    }
    with(change) {
      return new Uri(
        change.scheme ?? this.scheme,
        change.authority ?? this.authority,
        change.path ?? this.path,
        change.query ?? this.query,
        change.fragment ?? this.fragment,
      );
    }
    toString() {
      const encodePath = (value) => value.split("/").map(encodeURIComponent).join("/");
      let text = `${this.scheme}:`;
      if (this.authority || this.scheme === "file") text += `//${encodeURIComponent(this.authority)}`;
      text += encodePath(this.path);
      if (this.query) text += `?${encodeURIComponent(this.query)}`;
      if (this.fragment) text += `#${encodeURIComponent(this.fragment)}`;
      return text;
    }
    toJSON() {
      return { $mid: 1, scheme: this.scheme, authority: this.authority, path: this.path, query: this.query, fragment: this.fragment };
    }
  }

  function toUri(value) {
    if (value instanceof Uri) return value;
    if (typeof value === "string") return Uri.parse(value);
    return Uri.from(value);
  }

  class ThemeColor {
    constructor(id) {
      this.id = id;
    }
  }

  class ThemeIcon {
    constructor(id, color) {
      this.id = id;
      this.color = color;
    }
  }

  class MarkdownString {
    constructor(value = "", supportThemeIcons = false) {
      this.value = value;
      this.supportThemeIcons = supportThemeIcons;
    }
    appendText(value) {
      this.value += value.replace(/[\\`*_{}[\]()#+\-.!]/g, "\\$&");
      return this;
    }
    appendMarkdown(value) {
      this.value += value;
      return this;
    }
    appendCodeblock(code, language = "") {
      this.value += `\n\`\`\`${language}\n${code}\n\`\`\`\n`;
      return this;
    }
  }

  class TextEdit {
    constructor(range, newText) {
      this.range = range;
      this.newText = newText;
    }
    static replace(range, newText) {
      return new TextEdit(range, newText);
    }
    static insert(position, newText) {
      return new TextEdit(new Range(position, position), newText);
    }
    static delete(range) {
      return new TextEdit(range, "");
    }
  }

  class WorkspaceEdit {
    constructor() {
      this._edits = new Map();
    }
    _list(uri) {
      const key = uri.toString();
      if (!this._edits.has(key)) this._edits.set(key, { uri, edits: [] });
      return this._edits.get(key).edits;
    }
    replace(uri, range, newText) {
      this._list(uri).push(new TextEdit(range, newText));
    }
    insert(uri, position, newText) {
      this._list(uri).push(TextEdit.insert(position, newText));
    }
    delete(uri, range) {
      this._list(uri).push(TextEdit.delete(range));
    }
    set(uri, edits) {
      this._edits.set(uri.toString(), { uri, edits: [...edits] });
    }
    get(uri) {
      return this._edits.get(uri.toString())?.edits ?? [];
    }
    has(uri) {
      return this._edits.has(uri.toString());
    }
    entries() {
      return [...this._edits.values()].map(({ uri, edits }) => [uri, edits]);
    }
    get size() {
      return this._edits.size;
    }
  }

  class CompletionItem {
    constructor(label, kind) {
      this.label = label;
      this.kind = kind;
    }
  }

  class CompletionList {
    constructor(items = [], isIncomplete = false) {
      this.items = items;
      this.isIncomplete = isIncomplete;
    }
  }

  class FileSystemError extends Error {
    constructor(messageOrUri, code = "Unknown") {
      super(messageOrUri instanceof Uri ? messageOrUri.toString() : String(messageOrUri ?? code));
      this.name = `${code} (FileSystemError)`;
      this.code = code;
    }
    static FileNotFound(messageOrUri) {
      return new FileSystemError(messageOrUri, "FileNotFound");
    }
    static FileExists(messageOrUri) {
      return new FileSystemError(messageOrUri, "FileExists");
    }
    static FileNotADirectory(messageOrUri) {
      return new FileSystemError(messageOrUri, "FileNotADirectory");
    }
    static FileIsADirectory(messageOrUri) {
      return new FileSystemError(messageOrUri, "FileIsADirectory");
    }
    static NoPermissions(messageOrUri) {
      return new FileSystemError(messageOrUri, "NoPermissions");
    }
    static Unavailable(messageOrUri) {
      return new FileSystemError(messageOrUri, "Unavailable");
    }
  }

  class TabInputText {
    constructor(uri) {
      this.uri = uri;
    }
  }

  // ---------------------------------------------------------------------
  // Enums

  const ViewColumn = { Active: -1, Beside: -2, One: 1, Two: 2, Three: 3, Four: 4, Five: 5, Six: 6, Seven: 7, Eight: 8, Nine: 9 };
  const StatusBarAlignment = { Left: 1, Right: 2 };
  const DecorationRangeBehavior = { OpenOpen: 0, ClosedClosed: 1, OpenClosed: 2, ClosedOpen: 3 };
  const TextEditorRevealType = { Default: 0, InCenter: 1, InCenterIfOutsideViewport: 2, AtTop: 3 };
  const FileType = { Unknown: 0, File: 1, Directory: 2, SymbolicLink: 64 };
  const FileChangeType = { Changed: 1, Created: 2, Deleted: 3 };
  const ConfigurationTarget = { Global: 1, Workspace: 2, WorkspaceFolder: 3 };
  const EndOfLine = { LF: 1, CRLF: 2 };
  const ExtensionMode = { Production: 1, Development: 2, Test: 3 };
  const CompletionItemKind = {
    Text: 0, Method: 1, Function: 2, Constructor: 3, Field: 4, Variable: 5, Class: 6, Interface: 7,
    Module: 8, Property: 9, Unit: 10, Value: 11, Enum: 12, Keyword: 13, Snippet: 14, Color: 15,
    File: 16, Reference: 17, Folder: 18, EnumMember: 19, Constant: 20, Struct: 21, Event: 22,
    Operator: 23, TypeParameter: 24, User: 25, Issue: 26,
  };
  const CompletionTriggerKind = { Invoke: 0, TriggerCharacter: 1, TriggerForIncompleteCompletions: 2 };
  const TextDocumentChangeReason = { Undo: 1, Redo: 2 };
  const ProgressLocation = { SourceControl: 1, Window: 10, Notification: 15 };

  // ---------------------------------------------------------------------
  // Values sent to Keel: classes become plain JSON

  function plain(value) {
    if (value instanceof ThemeColor) return { themeColor: value.id };
    if (value instanceof Uri) return value.toString();
    if (value instanceof Position) return { line: value.line, character: value.character };
    if (value instanceof Range) return { start: plain(value.start), end: plain(value.end) };
    if (Array.isArray(value)) return value.map(plain);
    if (value && typeof value === "object") {
      const result = {};
      for (const [key, inner] of Object.entries(value)) result[key] = plain(inner);
      return result;
    }
    return value;
  }

  // ---------------------------------------------------------------------
  // Extensions and their activation

  const extensions = new Map();
  const configurationDefaults = new Map();
  const configurationValues = new Map();

  function createMemento() {
    const values = new Map();
    return {
      get: (key, defaultValue) => (values.has(key) ? values.get(key) : defaultValue),
      update: async (key, value) => {
        if (value === undefined) values.delete(key);
        else values.set(key, value);
      },
      keys: () => [...values.keys()],
      setKeysForSync: () => {},
    };
  }

  function createContext(extension) {
    const path = globalThis.__keel.builtin("path");
    const storage = extension.storagePath;
    return {
      subscriptions: [],
      extensionPath: extension.root,
      extensionUri: Uri.file(extension.root),
      globalStorageUri: Uri.file(path.join(storage, "global")),
      storageUri: Uri.file(path.join(storage, "workspace")),
      logUri: Uri.file(path.join(storage, "logs")),
      globalStoragePath: path.join(storage, "global"),
      storagePath: path.join(storage, "workspace"),
      logPath: path.join(storage, "logs"),
      globalState: createMemento(),
      workspaceState: createMemento(),
      secrets: {
        get: async () => undefined,
        store: async () => {},
        delete: async () => {},
        onDidChange: new EventEmitter().event,
      },
      extension: extensionObject(extension),
      extensionMode: ExtensionMode.Production,
      asAbsolutePath: (relative) => path.join(extension.root, relative),
      environmentVariableCollection: { replace() {}, append() {}, prepend() {}, get() {}, delete() {}, clear() {} },
    };
  }

  function extensionObject(extension) {
    return {
      id: extension.id,
      extensionPath: extension.root,
      extensionUri: Uri.file(extension.root),
      packageJSON: extension.packageJson,
      get isActive() {
        return extension.active;
      },
      get exports() {
        return extension.exports;
      },
      activate: () => activate(extension.id),
    };
  }

  async function activate(id) {
    const extension = extensions.get(id);
    if (!extension) throw new Error(`extension ${id} is not loaded`);
    if (extension.active) return extension.exports;
    if (extension.activating) return extension.activating;
    extension.activating = (async () => {
      try {
        const path = globalThis.__keel.builtin("path");
        const main = extension.main ? globalThis.__keel.requireFile(path.join(extension.root, extension.main)) : {};
        extension.context = createContext(extension);
        extension.exports = (await main.activate?.(extension.context)) ?? main;
        extension.module = main;
        extension.active = true;
        host.notify("extensions/activated", { id });
        return extension.exports;
      } catch (error) {
        host.notify("extensions/activationFailed", { id, error: host.describe(error) });
        throw error;
      } finally {
        extension.activating = undefined;
      }
    })();
    return extension.activating;
  }

  host.onCall("extensions/load", (params) => {
    const extension = { ...params, active: false, exports: undefined };
    extensions.set(params.id, extension);
    for (const [key, value] of Object.entries(params.configurationDefaults ?? {})) {
      configurationDefaults.set(key, value);
    }
    return true;
  });

  host.onCall("extensions/activate", async ({ id }) => {
    await activate(id);
    return true;
  });

  host.onCall("extensions/deactivate", async ({ id }) => {
    const extension = extensions.get(id);
    if (!extension?.active) return false;
    try {
      await extension.module?.deactivate?.();
    } finally {
      for (const disposable of extension.context.subscriptions) {
        try {
          disposable?.dispose?.();
        } catch (error) {
          console.error(`disposing a subscription of ${id} failed:`, error);
        }
      }
      extension.active = false;
    }
    return true;
  });

  // ---------------------------------------------------------------------
  // Commands

  const commands = new Map();

  function extensionActivatedBy(command) {
    for (const extension of extensions.values()) {
      if (extension.active) continue;
      const events = extension.packageJson?.activationEvents ?? [];
      if (events.includes("*") || events.includes(`onCommand:${command}`)) return extension;
    }
    return undefined;
  }

  async function executeCommand(command, ...args) {
    if (!commands.has(command)) {
      const extension = extensionActivatedBy(command);
      if (extension) await activate(extension.id);
    }
    const registered = commands.get(command);
    if (registered) return registered.callback.apply(registered.thisArg, args);
    return host.call("commands/executeHost", { command, args: plain(args) });
  }

  const commandsApi = {
    registerCommand(command, callback, thisArg) {
      if (commands.has(command)) throw new Error(`command '${command}' already exists`);
      commands.set(command, { callback, thisArg });
      host.notify("commands/registered", { command });
      return new Disposable(() => {
        commands.delete(command);
        host.notify("commands/unregistered", { command });
      });
    },
    registerTextEditorCommand(command, callback, thisArg) {
      return commandsApi.registerCommand(command, (...args) => {
        const editor = windowApi.activeTextEditor;
        if (editor) return callback.call(thisArg, editor, undefined, ...args);
      });
    },
    executeCommand,
    getCommands: async () => [...commands.keys()],
  };

  host.onCall("commands/execute", ({ command, args }) => executeCommand(command, ...(args ?? [])));

  // ---------------------------------------------------------------------
  // Configuration

  const configurationChanged = new EventEmitter();

  function getConfiguration(section) {
    const prefix = section ? `${section}.` : "";
    const lookup = (key) => {
      const full = prefix + key;
      if (configurationValues.has(full)) return { found: true, value: configurationValues.get(full) };
      if (configurationDefaults.has(full)) return { found: true, value: configurationDefaults.get(full) };
      return { found: false, value: undefined };
    };
    return {
      get(key, defaultValue) {
        const { found, value } = lookup(key);
        return found ? structuredCopy(value) : defaultValue;
      },
      has: (key) => lookup(key).found,
      inspect(key) {
        const full = prefix + key;
        return {
          key: full,
          defaultValue: configurationDefaults.get(full),
          globalValue: configurationValues.get(full),
        };
      },
      async update(key, value) {
        const full = prefix + key;
        if (value === undefined) configurationValues.delete(full);
        else configurationValues.set(full, value);
        await host.call("configuration/update", { key: full, value: value ?? null });
        configurationChanged.fire({ affectsConfiguration: (name) => full === name || full.startsWith(`${name}.`) });
      },
    };
  }

  function structuredCopy(value) {
    return value === undefined ? undefined : JSON.parse(JSON.stringify(value));
  }

  host.onNotify("configuration/changed", ({ values }) => {
    const changed = Object.keys(values);
    for (const [key, value] of Object.entries(values)) {
      if (value === null) configurationValues.delete(key);
      else configurationValues.set(key, value);
    }
    configurationChanged.fire({
      affectsConfiguration: (name) => changed.some((key) => key === name || key.startsWith(`${name}.`)),
    });
  });

  // ---------------------------------------------------------------------
  // File system

  const fileSystemProviders = new Map();
  const ops = Deno.core.ops;

  function fileSystemError(error, uri) {
    const message = String(error?.message ?? error);
    for (const code of ["FileNotFound", "FileExists", "FileNotADirectory", "FileIsADirectory", "NoPermissions"]) {
      if (message.includes(code)) return new FileSystemError(uri, code);
    }
    return new FileSystemError(message, "Unknown");
  }

  function onDisk(uri, operation) {
    try {
      return operation(uri.fsPath);
    } catch (error) {
      throw fileSystemError(error, uri);
    }
  }

  const fs = {
    async readFile(uri) {
      uri = toUri(uri);
      const provider = fileSystemProviders.get(uri.scheme);
      if (provider) return provider.readFile(uri);
      return onDisk(uri, (file) => ops.op_keel_fs_read(file));
    },
    async writeFile(uri, content) {
      uri = toUri(uri);
      const provider = fileSystemProviders.get(uri.scheme);
      if (provider) return provider.writeFile(uri, content, { create: true, overwrite: true });
      return onDisk(uri, (file) => ops.op_keel_fs_write(file, content));
    },
    async stat(uri) {
      uri = toUri(uri);
      const provider = fileSystemProviders.get(uri.scheme);
      if (provider) return provider.stat(uri);
      return onDisk(uri, (file) => ops.op_keel_fs_stat(file));
    },
    async readDirectory(uri) {
      uri = toUri(uri);
      const provider = fileSystemProviders.get(uri.scheme);
      if (provider) return provider.readDirectory(uri);
      return onDisk(uri, (file) => ops.op_keel_fs_read_directory(file));
    },
    async createDirectory(uri) {
      uri = toUri(uri);
      const provider = fileSystemProviders.get(uri.scheme);
      if (provider) return provider.createDirectory(uri);
      return onDisk(uri, (file) => ops.op_keel_fs_create_directory(file));
    },
    async delete(uri, options = {}) {
      uri = toUri(uri);
      const provider = fileSystemProviders.get(uri.scheme);
      if (provider) return provider.delete(uri, { recursive: !!options.recursive });
      return onDisk(uri, (file) => ops.op_keel_fs_delete(file, !!options.recursive));
    },
    async rename(source, target, options = {}) {
      source = toUri(source);
      target = toUri(target);
      const provider = fileSystemProviders.get(source.scheme);
      if (provider) return provider.rename(source, target, { overwrite: !!options.overwrite });
      return onDisk(source, (file) => ops.op_keel_fs_rename(file, target.fsPath, !!options.overwrite));
    },
  };

  // ---------------------------------------------------------------------
  // Documents and editors: Keel keeps them in sync from phase 0.5 on

  const documentOpened = new EventEmitter();
  const documentClosed = new EventEmitter();
  const documentChanged = new EventEmitter();
  const documentSaved = new EventEmitter();
  const activeEditorChanged = new EventEmitter();
  const visibleEditorsChanged = new EventEmitter();
  const selectionChanged = new EventEmitter();
  const visibleRangesChanged = new EventEmitter();
  const tabsChanged = new EventEmitter();
  const tabGroupsChanged = new EventEmitter();

  const state = {
    documents: [],
    visibleEditors: [],
    activeEditor: undefined,
    tabGroups: { all: [], activeTabGroup: { tabs: [], viewColumn: ViewColumn.One, isActive: true, activeTab: undefined } },
  };

  // ---------------------------------------------------------------------
  // Window

  function splitMessageItems(items) {
    let options = {};
    if (items.length && items[0] && typeof items[0] === "object" && !("title" in items[0])) {
      options = items.shift();
    }
    return { options, items };
  }

  function showMessage(severity) {
    return async (message, ...rest) => {
      const { options, items } = splitMessageItems(rest);
      const titles = items.map((item) => (typeof item === "string" ? item : item.title));
      const chosen = await host.call("window/showMessage", { severity, message, items: titles, modal: !!options.modal, detail: options.detail ?? null });
      return typeof chosen === "number" ? items[chosen] : undefined;
    };
  }

  let nextStatusItemId = 1;
  function createStatusBarItem(idOrAlignment, alignmentOrPriority, maybePriority) {
    const id = typeof idOrAlignment === "string" ? idOrAlignment : `status-${nextStatusItemId++}`;
    const alignment = typeof idOrAlignment === "string" ? alignmentOrPriority : idOrAlignment;
    const priority = typeof idOrAlignment === "string" ? maybePriority : alignmentOrPriority;
    const item = {
      id,
      alignment: alignment ?? StatusBarAlignment.Left,
      priority,
      text: "",
      tooltip: undefined,
      command: undefined,
      color: undefined,
      backgroundColor: undefined,
      name: undefined,
      visible: false,
    };
    let scheduled = false;
    const publish = () => {
      if (scheduled) return;
      scheduled = true;
      queueMicrotask(() => {
        scheduled = false;
        host.notify("statusBar/update", plain({ ...item }));
      });
    };
    return new Proxy(item, {
      get(target, key) {
        if (key === "show") return () => ((target.visible = true), publish());
        if (key === "hide") return () => ((target.visible = false), publish());
        if (key === "dispose") return () => host.notify("statusBar/remove", { id });
        return target[key];
      },
      set(target, key, value) {
        target[key] = value;
        publish();
        return true;
      },
    });
  }

  let nextDecorationTypeId = 1;

  const windowApi = {
    get activeTextEditor() {
      return state.activeEditor;
    },
    get visibleTextEditors() {
      return state.visibleEditors;
    },
    get tabGroups() {
      return {
        all: state.tabGroups.all,
        activeTabGroup: state.tabGroups.activeTabGroup,
        onDidChangeTabs: tabsChanged.event,
        onDidChangeTabGroups: tabGroupsChanged.event,
        close: (tabs, preserveFocus) => host.call("window/closeTabs", { tabs: plain([tabs].flat()), preserveFocus: !!preserveFocus }),
      };
    },
    onDidChangeActiveTextEditor: activeEditorChanged.event,
    onDidChangeVisibleTextEditors: visibleEditorsChanged.event,
    onDidChangeTextEditorSelection: selectionChanged.event,
    onDidChangeTextEditorVisibleRanges: visibleRangesChanged.event,
    showInformationMessage: showMessage("info"),
    showWarningMessage: showMessage("warning"),
    showErrorMessage: showMessage("error"),
    setStatusBarMessage(text, hideAfter) {
      const timeout = typeof hideAfter === "number" ? hideAfter : null;
      host.notify("window/statusBarMessage", { text, timeout });
      const clear = new Disposable(() => host.notify("window/statusBarMessage", { text: "", timeout: null }));
      if (hideAfter && typeof hideAfter.then === "function") hideAfter.then(() => clear.dispose());
      return clear;
    },
    createStatusBarItem,
    createTextEditorDecorationType(options) {
      const key = `decoration-${nextDecorationTypeId++}`;
      host.notify("decorations/createType", { key, options: plain(options) });
      return { key, dispose: () => host.notify("decorations/disposeType", { key }) };
    },
    async showQuickPick(items, options = {}) {
      const list = await items;
      const labels = list.map((item) => (typeof item === "string" ? { label: item } : plain(item)));
      const chosen = await host.call("window/showQuickPick", { items: labels, options: plain(options) });
      if (chosen == null) return undefined;
      return Array.isArray(chosen) ? chosen.map((index) => list[index]) : list[chosen];
    },
    async showInputBox(options = {}) {
      const value = await host.call("window/showInputBox", plain(options));
      return value ?? undefined;
    },
    async showTextDocument(documentOrUri, columnOrOptions, preserveFocus) {
      const uri = documentOrUri instanceof Uri ? documentOrUri : documentOrUri.uri;
      const options = typeof columnOrOptions === "object" ? columnOrOptions : { viewColumn: columnOrOptions, preserveFocus };
      await host.call("window/showTextDocument", plain({ uri, ...options }));
      return state.visibleEditors.find((editor) => editor.document.uri.toString() === uri.toString()) ?? state.activeEditor;
    },
    createOutputChannel(name) {
      const write = (text) => host.notify("output/append", { name, text });
      return {
        name,
        append: write,
        appendLine: (text) => write(`${text}\n`),
        replace: (text) => host.notify("output/replace", { name, text }),
        clear: () => host.notify("output/replace", { name, text: "" }),
        show() {},
        hide() {},
        dispose() {},
      };
    },
    async withProgress(_options, task) {
      return task({ report() {} }, noCancellation);
    },
  };

  // ---------------------------------------------------------------------
  // Workspace

  const contentProviders = new Map();

  const workspaceApi = {
    get textDocuments() {
      return state.documents;
    },
    get workspaceFolders() {
      return state.workspaceFolders;
    },
    getConfiguration,
    onDidChangeConfiguration: configurationChanged.event,
    onDidOpenTextDocument: documentOpened.event,
    onDidCloseTextDocument: documentClosed.event,
    onDidChangeTextDocument: documentChanged.event,
    onDidSaveTextDocument: documentSaved.event,
    fs,
    registerFileSystemProvider(scheme, provider, options = {}) {
      fileSystemProviders.set(scheme, provider);
      host.notify("fileSystem/registered", { scheme, readonly: !!options.isReadonly });
      return new Disposable(() => {
        fileSystemProviders.delete(scheme);
        host.notify("fileSystem/unregistered", { scheme });
      });
    },
    registerTextDocumentContentProvider(scheme, provider) {
      contentProviders.set(scheme, provider);
      host.notify("contentProvider/registered", { scheme });
      const subscription = provider.onDidChange?.((uri) => host.notify("contentProvider/changed", { uri: uri.toString() }));
      return new Disposable(() => {
        subscription?.dispose();
        contentProviders.delete(scheme);
        host.notify("contentProvider/unregistered", { scheme });
      });
    },
    async openTextDocument(uriOrOptions) {
      const params = uriOrOptions instanceof Uri || typeof uriOrOptions === "string"
        ? { uri: toUri(uriOrOptions).toString() }
        : plain(uriOrOptions ?? {});
      const opened = await host.call("workspace/openTextDocument", params);
      return state.documents.find((document) => document.uri.toString() === opened) ?? undefined;
    },
    async applyEdit(edit) {
      const entries = edit.entries().map(([uri, edits]) => ({ uri: uri.toString(), edits: plain(edits) }));
      return (await host.call("workspace/applyEdit", { entries })) === true;
    },
    asRelativePath(pathOrUri) {
      const value = pathOrUri instanceof Uri ? pathOrUri.fsPath : String(pathOrUri);
      return value;
    },
  };

  // ---------------------------------------------------------------------
  // Languages

  const completionProviders = [];
  let nextProviderId = 1;

  const languagesApi = {
    registerCompletionItemProvider(selector, provider, ...triggerCharacters) {
      const entry = { id: nextProviderId++, selector: plain(selector), provider, triggerCharacters };
      completionProviders.push(entry);
      host.notify("completion/registered", { id: entry.id, selector: entry.selector, triggerCharacters });
      return new Disposable(() => {
        completionProviders.splice(completionProviders.indexOf(entry), 1);
        host.notify("completion/unregistered", { id: entry.id });
      });
    },
    async setTextDocumentLanguage(document, languageId) {
      await host.call("languages/setTextDocumentLanguage", { uri: document.uri.toString(), languageId });
      return document;
    },
    getLanguages: async () => ["typescript", "typescriptreact", "javascript", "javascriptreact", "plaintext"],
    match: () => 10,
  };

  // ---------------------------------------------------------------------
  // Source control: VSAuto Ghost uses it for the green "added lines" bar

  const sourceControls = new Map();

  const scmApi = {
    createSourceControl(id, label, rootUri) {
      const control = {
        id,
        label,
        rootUri,
        inputBox: { value: "", placeholder: "", visible: false },
        count: undefined,
        commitTemplate: undefined,
        acceptInputCommand: undefined,
        statusBarCommands: undefined,
        _quickDiffProvider: undefined,
        get quickDiffProvider() {
          return this._quickDiffProvider;
        },
        set quickDiffProvider(provider) {
          this._quickDiffProvider = provider;
          host.notify("scm/quickDiffProvider", { id, active: !!provider });
        },
        createResourceGroup(groupId, groupLabel) {
          return { id: groupId, label: groupLabel, resourceStates: [], hideWhenEmpty: false, dispose() {} };
        },
        dispose() {
          sourceControls.delete(id);
          host.notify("scm/disposed", { id });
        },
      };
      sourceControls.set(id, control);
      host.notify("scm/created", { id, label, rootUri: rootUri?.toString() ?? null });
      return control;
    },
  };

  // ---------------------------------------------------------------------
  // Environment

  const envApi = {
    appName: "Keel",
    appRoot: "",
    appHost: "desktop",
    language: "it",
    machineId: "keel",
    sessionId: "keel-session",
    uriScheme: "keel",
    shell: "/bin/zsh",
    uiKind: 1,
    clipboard: {
      readText: async () => (await host.call("env/readClipboard")) ?? "",
      writeText: (text) => host.call("env/writeClipboard", { text }),
    },
    openExternal: async (uri) => (await host.call("env/openExternal", { uri: uri.toString() })) === true,
  };

  const l10nApi = {
    t(message, ...args) {
      const text = typeof message === "string" ? message : message.message;
      const values = typeof message === "string" ? args : message.args ?? [];
      const named = values.length === 1 && values[0] && typeof values[0] === "object" ? values[0] : null;
      return text.replace(/\{(\w+)\}/g, (match, key) => {
        const value = named ? named[key] : values[Number(key)];
        return value === undefined ? match : String(value);
      });
    },
    bundle: undefined,
    uri: undefined,
  };

  // ---------------------------------------------------------------------
  // The module

  const vscode = {
    version: "1.95.0",
    Disposable,
    EventEmitter,
    CancellationTokenSource,
    CancellationToken: { None: noCancellation },
    Position,
    Range,
    Selection,
    Uri,
    ThemeColor,
    ThemeIcon,
    MarkdownString,
    TextEdit,
    WorkspaceEdit,
    CompletionItem,
    CompletionList,
    FileSystemError,
    TabInputText,
    ViewColumn,
    StatusBarAlignment,
    DecorationRangeBehavior,
    TextEditorRevealType,
    FileType,
    FileChangeType,
    ConfigurationTarget,
    EndOfLine,
    ExtensionMode,
    CompletionItemKind,
    CompletionTriggerKind,
    TextDocumentChangeReason,
    ProgressLocation,
    commands: commandsApi,
    window: windowApi,
    workspace: workspaceApi,
    languages: languagesApi,
    scm: scmApi,
    env: envApi,
    l10n: l10nApi,
    extensions: {
      getExtension: (id) => {
        const extension = extensions.get(id);
        return extension ? extensionObject(extension) : undefined;
      },
      get all() {
        return [...extensions.values()].map(extensionObject);
      },
      onDidChange: new EventEmitter().event,
    },
  };

  globalThis.__keel.vscode = vscode;
  globalThis.__keel.internals = {
    state,
    commands,
    completionProviders,
    contentProviders,
    fileSystemProviders,
    sourceControls,
    emitters: {
      documentOpened,
      documentClosed,
      documentChanged,
      documentSaved,
      activeEditorChanged,
      visibleEditorsChanged,
      selectionChanged,
      visibleRangesChanged,
      tabsChanged,
      tabGroupsChanged,
    },
  };
})(globalThis);
