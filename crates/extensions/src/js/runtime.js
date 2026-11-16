// The JavaScript side of Keel's extension host.
//
// It provides the small part of Node.js that extensions rely on (`require`,
// `Buffer`, timers, `console`, `path`, `crypto`) and the message channel to
// Keel. The `vscode` module itself lives in vscode.js.
"use strict";

((globalThis) => {
  const core = Deno.core;
  const ops = core.ops;

  // ---------------------------------------------------------------------
  // Messages to and from Keel

  let nextCallId = 1;
  const pendingCalls = new Map();
  const callHandlers = new Map();
  const notifyHandlers = new Map();

  function post(message) {
    let text;
    try {
      text = JSON.stringify(message);
    } catch (error) {
      text = JSON.stringify({ ...message, result: null, params: null, error: describe(error) });
    }
    ops.op_keel_post(text);
  }

  function describe(error) {
    if (error instanceof Error) {
      return error.stack || `${error.name}: ${error.message}`;
    }
    return String(error);
  }

  const host = {
    notify(method, params = null) {
      post({ type: "notify", method, params });
    },
    call(method, params = null) {
      const id = nextCallId++;
      return new Promise((resolve, reject) => {
        pendingCalls.set(id, { resolve, reject });
        post({ type: "call", id, method, params });
      });
    },
    onCall(method, handler) {
      callHandlers.set(method, handler);
    },
    onNotify(method, handler) {
      notifyHandlers.set(method, handler);
    },
    describe,
  };

  // Answered at once: Keel asks it to tell a busy host from a stuck one.
  callHandlers.set("host/ping", () => "pong");

  async function handle(message) {
    switch (message.type) {
      case "reply": {
        const pending = pendingCalls.get(message.id);
        if (!pending) return;
        pendingCalls.delete(message.id);
        if (message.error != null) pending.reject(new Error(message.error));
        else pending.resolve(message.result ?? undefined);
        return;
      }
      case "notify": {
        const handler = notifyHandlers.get(message.method);
        if (!handler) return;
        try {
          await handler(message.params);
        } catch (error) {
          console.error(`notification ${message.method} failed:`, error);
        }
        return;
      }
      case "call": {
        try {
          const handler = callHandlers.get(message.method);
          if (!handler) throw new Error(`unknown method: ${message.method}`);
          const result = await handler(message.params);
          post({ type: "reply", id: message.id, result: result ?? null });
        } catch (error) {
          post({ type: "reply", id: message.id, result: null, error: describe(error) });
        }
        return;
      }
    }
  }

  async function messageLoop() {
    for (;;) {
      const raw = await ops.op_keel_next();
      if (raw == null) return;
      // Not awaited: a handler may wait for another message from Keel.
      handle(JSON.parse(raw));
    }
  }

  // ---------------------------------------------------------------------
  // Console

  function formatValue(value) {
    if (typeof value === "string") return value;
    if (value instanceof Error) return describe(value);
    try {
      return JSON.stringify(value);
    } catch {
      return String(value);
    }
  }

  function logger(level) {
    return (...values) => host.notify("log", { level, message: values.map(formatValue).join(" ") });
  }

  globalThis.console = {
    log: logger("info"),
    info: logger("info"),
    debug: logger("debug"),
    warn: logger("warn"),
    error: logger("error"),
    trace: logger("debug"),
  };

  // ---------------------------------------------------------------------
  // Timers

  // An extension's error must not stop the host and the other extensions:
  // errors in timers and promises nobody handles go to the log instead.
  function reportError(error) {
    host.notify("log", { level: "error", message: `An extension failed: ${describe(error)}` });
  }
  core.setUnhandledPromiseRejectionHandler((_promise, reason) => {
    reportError(reason);
    return true;
  });
  const guarded = (callback, args) => () => {
    try {
      callback(...args);
    } catch (error) {
      reportError(error);
    }
  };

  globalThis.setTimeout = (callback, delay, ...args) =>
    core.createTimer(guarded(callback, args), delay, undefined, false, true);
  globalThis.setInterval = (callback, delay, ...args) =>
    core.createTimer(guarded(callback, args), delay, undefined, true, true);
  globalThis.clearTimeout = (timer) => {
    if (timer) core.cancelTimer(timer);
  };
  globalThis.clearInterval = globalThis.clearTimeout;
  globalThis.setImmediate = (callback, ...args) => globalThis.setTimeout(callback, 0, ...args);
  globalThis.clearImmediate = globalThis.clearTimeout;
  const nativeQueueMicrotask = globalThis.queueMicrotask;
  // Guarded like timers below: an error here would stop the whole host.
  globalThis.queueMicrotask = (callback) => {
    const run = () => {
      try {
        callback();
      } catch (error) {
        host.notify("log", { level: "error", message: `An extension failed: ${describe(error)}` });
      }
    };
    if (nativeQueueMicrotask) nativeQueueMicrotask(run);
    else Promise.resolve().then(run);
  };

  // ---------------------------------------------------------------------
  // Buffer: just enough for UTF-8 text and bytes

  class Buffer extends Uint8Array {
    static from(value, encoding) {
      if (typeof value === "string") {
        checkEncoding(encoding);
        const bytes = core.encode(value);
        return new Buffer(bytes.buffer, bytes.byteOffset, bytes.byteLength);
      }
      if (value instanceof ArrayBuffer) {
        return new Buffer(value.slice(0));
      }
      if (ArrayBuffer.isView(value) || Array.isArray(value)) {
        const copy = new Buffer(value.length);
        copy.set(value);
        return copy;
      }
      throw new TypeError("Buffer.from: unsupported value");
    }
    static alloc(size) {
      return new Buffer(size);
    }
    static byteLength(value, encoding) {
      if (typeof value !== "string") return value.byteLength;
      checkEncoding(encoding);
      return core.encode(value).byteLength;
    }
    static isBuffer(value) {
      return value instanceof Buffer;
    }
    static concat(list, totalLength) {
      const size = totalLength ?? list.reduce((sum, part) => sum + part.length, 0);
      const joined = new Buffer(size);
      let at = 0;
      for (const part of list) {
        if (at >= size) break;
        joined.set(part.subarray(0, size - at), at);
        at += part.length;
      }
      return joined;
    }
    static isEncoding(encoding) {
      return /^utf-?8$/i.test(String(encoding));
    }
    toString(encoding) {
      checkEncoding(encoding);
      return core.decode(this);
    }
  }

  function checkEncoding(encoding) {
    if (encoding != null && !/^utf-?8$/i.test(encoding)) {
      throw new Error(`Keel supports only UTF-8 buffers, not ${encoding}`);
    }
  }

  globalThis.Buffer = Buffer;

  // ---------------------------------------------------------------------
  // process: present but not Node-like, so libraries such as TypeScript do
  // not try to load Node's file system modules.

  globalThis.process = {
    platform: "darwin",
    env: {},
    argv: [],
    versions: {},
    cwd: () => "/",
  };

  // ---------------------------------------------------------------------
  // path (POSIX)

  function normalizeParts(parts, absolute) {
    const result = [];
    for (const part of parts) {
      if (!part || part === ".") continue;
      if (part === "..") {
        if (result.length && result[result.length - 1] !== "..") result.pop();
        else if (!absolute) result.push("..");
      } else {
        result.push(part);
      }
    }
    return result;
  }

  const path = {
    sep: "/",
    delimiter: ":",
    normalize(value) {
      const absolute = value.startsWith("/");
      const joined = normalizeParts(value.split("/"), absolute).join("/");
      return (absolute ? "/" : "") + (joined || (absolute ? "" : "."));
    },
    join(...parts) {
      return path.normalize(parts.filter((part) => part !== "").join("/"));
    },
    resolve(...parts) {
      let resolved = "";
      for (let index = parts.length - 1; index >= 0 && !resolved.startsWith("/"); index--) {
        resolved = parts[index] + (resolved ? "/" + resolved : "");
      }
      return path.normalize(resolved.startsWith("/") ? resolved : "/" + resolved);
    },
    isAbsolute: (value) => value.startsWith("/"),
    dirname(value) {
      const trimmed = value.replace(/\/+$/, "");
      const index = trimmed.lastIndexOf("/");
      if (index < 0) return ".";
      return index === 0 ? "/" : trimmed.slice(0, index);
    },
    basename(value, extension) {
      const base = value.replace(/\/+$/, "").split("/").pop() ?? "";
      return extension && base.endsWith(extension) && base !== extension
        ? base.slice(0, -extension.length)
        : base;
    },
    extname(value) {
      const base = path.basename(value);
      const index = base.lastIndexOf(".");
      return index <= 0 ? "" : base.slice(index);
    },
    relative(from, to) {
      const fromParts = path.resolve(from).split("/").filter(Boolean);
      const toParts = path.resolve(to).split("/").filter(Boolean);
      let common = 0;
      while (common < fromParts.length && fromParts[common] === toParts[common]) common++;
      return [...fromParts.slice(common).map(() => ".."), ...toParts.slice(common)].join("/");
    },
  };
  path.posix = path;

  // ---------------------------------------------------------------------
  // crypto: SHA-256 hashes

  const crypto = {
    createHash(algorithm) {
      if (String(algorithm).toLowerCase() !== "sha256") {
        throw new Error(`Keel supports only sha256 hashes, not ${algorithm}`);
      }
      let text = "";
      const hash = {
        update(value, encoding) {
          text += typeof value === "string" ? value : Buffer.from(value).toString(encoding);
          return hash;
        },
        digest(encoding) {
          if (encoding !== "hex") throw new Error("Keel supports only hex digests");
          return ops.op_keel_sha256(text);
        },
      };
      return hash;
    },
  };

  // ---------------------------------------------------------------------
  // CommonJS modules

  const builtins = new Map([
    ["path", () => path],
    ["crypto", () => crypto],
    ["buffer", () => ({ Buffer })],
    ["vscode", () => globalThis.__keel.vscode],
  ]);
  const moduleCache = new Map();

  function readModule(file) {
    return ops.op_keel_read_module(file);
  }

  function resolveFile(candidate) {
    for (const file of [candidate, `${candidate}.js`, `${candidate}.json`, `${candidate}/index.js`]) {
      const source = readModule(file);
      if (source != null) return { file, source };
    }
    return null;
  }

  function resolvePackage(name, fromDirectory) {
    let directory = fromDirectory;
    for (;;) {
      const base = path.join(directory, "node_modules", name);
      const manifest = readModule(path.join(base, "package.json"));
      if (manifest != null) {
        const main = JSON.parse(manifest).main ?? "index.js";
        const found = resolveFile(path.join(base, main));
        if (found) return found;
      }
      const found = resolveFile(base);
      if (found) return found;
      if (directory === "/") return null;
      directory = path.dirname(directory);
    }
  }

  function makeRequire(fromDirectory) {
    return function require(specifier) {
      const name = specifier.startsWith("node:") ? specifier.slice(5) : specifier;
      const builtin = builtins.get(name);
      if (builtin) return builtin();
      const found = name.startsWith(".") || name.startsWith("/")
        ? resolveFile(path.resolve(fromDirectory, name))
        : resolvePackage(name, fromDirectory);
      if (!found) throw new Error(`Cannot find module '${specifier}' from ${fromDirectory}`);
      return loadModule(found.file, found.source);
    };
  }

  function loadModule(file, source) {
    const cached = moduleCache.get(file);
    if (cached) return cached.exports;
    const module = { exports: {}, filename: file, loaded: false };
    moduleCache.set(file, module);
    if (file.endsWith(".json")) {
      module.exports = JSON.parse(source);
    } else {
      const body = source.startsWith("#!") ? "//" + source : source;
      const wrapper = (0, eval)(
        `(function (exports, require, module, __filename, __dirname) {${body}\n})\n//# sourceURL=${file}`,
      );
      wrapper.call(module.exports, module.exports, makeRequire(path.dirname(file)), module, file, path.dirname(file));
    }
    module.loaded = true;
    return module.exports;
  }

  function requireFile(file) {
    const found = resolveFile(file);
    if (!found) throw new Error(`Cannot find module ${file}`);
    return loadModule(found.file, found.source);
  }

  globalThis.__keel = {
    host,
    requireFile,
    messageLoop,
    builtin: (name) => builtins.get(name)(),
    // node.js adds `fs`, `os`, `events`... Each is built once, when first required.
    registerBuiltin: (name, factory) => {
      let made;
      builtins.set(name, () => (made ??= factory()));
    },
    vscode: undefined,
  };
})(globalThis);
