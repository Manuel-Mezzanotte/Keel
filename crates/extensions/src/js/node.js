// A small part of Node.js for extensions bundled for VS Code: `fs`, `os`,
// `events`, `util`, `url`, `stream`, `http`/`https` and a few stubs, with
// only what extensions such as markdownlint and gitignore use. The global
// `process` stays not Node-like (see runtime.js); `require("process")` gets
// a fuller one.
"use strict";

((globalThis) => {
  const ops = Deno.core.ops;
  const keel = globalThis.__keel;

  // Bundles written for Node use `global` and the standard streams of the
  // global `process`; neither makes it look like Node (no `nextTick`).
  globalThis.global ??= globalThis;
  for (const [name, fd] of [["stdout", 1], ["stderr", 2]]) {
    globalThis.process[name] ??= {
      fd,
      isTTY: false,
      write: (text) => {
        console.log(String(text).replace(/\n$/, ""));
        return true;
      },
      on: () => {},
      once: () => {},
    };
  }
  globalThis.process.stdin ??= { fd: 0, isTTY: false, on: () => {} };
  // Libraries such as fast-glob check the version for what `fs` offers.
  globalThis.process.versions.node ??= "20.18.0";
  globalThis.process.version ??= "v20.18.0";
  const register = keel.registerBuiltin;
  const path = () => keel.builtin("path");

  // ---------------------------------------------------------------------
  // Errors with Node's codes, from the native side's "ENOENT: ..." messages

  function nodeError(error) {
    const message = String(error?.message ?? error);
    const match = /^(E[A-Z]+|FileNotFound|FileExists|NoPermissions|FileNotADirectory|FileIsADirectory): /.exec(message);
    const codes = {
      FileNotFound: "ENOENT",
      FileExists: "EEXIST",
      NoPermissions: "EACCES",
      FileNotADirectory: "ENOTDIR",
      FileIsADirectory: "EISDIR",
    };
    const wrapped = new Error(message);
    wrapped.code = match ? codes[match[1]] ?? match[1] : "EIO";
    return wrapped;
  }

  function attempt(work) {
    try {
      return work();
    } catch (error) {
      throw nodeError(error);
    }
  }

  // ---------------------------------------------------------------------
  // events

  class EventEmitter {
    constructor() {
      this._events = new Map();
      this._maxListeners = EventEmitter.defaultMaxListeners;
    }
    _list(name) {
      if (!this._events) this._events = new Map();
      if (!this._events.has(name)) this._events.set(name, []);
      return this._events.get(name);
    }
    on(name, listener) {
      this._list(name).push(listener);
      return this;
    }
    addListener(name, listener) {
      return this.on(name, listener);
    }
    prependListener(name, listener) {
      this._list(name).unshift(listener);
      return this;
    }
    once(name, listener) {
      const wrapper = (...args) => {
        this.off(name, wrapper);
        listener.apply(this, args);
      };
      wrapper.listener = listener;
      return this.on(name, wrapper);
    }
    prependOnceListener(name, listener) {
      return this.once(name, listener);
    }
    off(name, listener) {
      const list = this._events?.get(name);
      if (list) {
        const index = list.findIndex((item) => item === listener || item.listener === listener);
        if (index >= 0) list.splice(index, 1);
      }
      return this;
    }
    removeListener(name, listener) {
      return this.off(name, listener);
    }
    removeAllListeners(name) {
      if (name === undefined) this._events = new Map();
      else this._events?.delete(name);
      return this;
    }
    emit(name, ...args) {
      const list = this._events?.get(name);
      if (!list || list.length === 0) {
        if (name === "error") throw args[0] instanceof Error ? args[0] : new Error(String(args[0]));
        return false;
      }
      for (const listener of [...list]) listener.apply(this, args);
      return true;
    }
    listeners(name) {
      return [...(this._events?.get(name) ?? [])].map((item) => item.listener ?? item);
    }
    rawListeners(name) {
      return [...(this._events?.get(name) ?? [])];
    }
    listenerCount(name) {
      return this._events?.get(name)?.length ?? 0;
    }
    eventNames() {
      return [...(this._events?.keys() ?? [])];
    }
    setMaxListeners(count) {
      this._maxListeners = count;
      return this;
    }
    getMaxListeners() {
      return this._maxListeners ?? EventEmitter.defaultMaxListeners;
    }
  }
  EventEmitter.defaultMaxListeners = 10;
  EventEmitter.EventEmitter = EventEmitter;
  EventEmitter.once = (emitter, name) =>
    new Promise((resolve, reject) => {
      emitter.once(name, (...args) => resolve(args));
      if (name !== "error") emitter.once("error", reject);
    });
  EventEmitter.on = (emitter, name) => {
    const queue = [];
    const waiting = [];
    emitter.on(name, (...args) => {
      if (waiting.length) waiting.shift()({ value: args, done: false });
      else queue.push(args);
    });
    return {
      [Symbol.asyncIterator]() {
        return this;
      },
      next: () =>
        queue.length ? Promise.resolve({ value: queue.shift(), done: false }) : new Promise((resolve) => waiting.push(resolve)),
      return: () => Promise.resolve({ value: undefined, done: true }),
    };
  };
  EventEmitter.setMaxListeners = () => {};
  EventEmitter.getEventListeners = (emitter, name) => emitter.listeners?.(name) ?? [];
  register("events", () => EventEmitter);

  // ---------------------------------------------------------------------
  // Text and URLs, which a bare V8 lacks

  if (!globalThis.TextEncoder) {
    globalThis.TextEncoder = class TextEncoder {
      get encoding() {
        return "utf-8";
      }
      encode(text = "") {
        return Deno.core.encode(String(text));
      }
    };
  }
  if (!globalThis.TextDecoder) {
    globalThis.TextDecoder = class TextDecoder {
      constructor(encoding = "utf-8") {
        if (!/^utf-?8$/i.test(encoding)) throw new RangeError(`Keel decodes only UTF-8, not ${encoding}`);
      }
      get encoding() {
        return "utf-8";
      }
      decode(bytes) {
        if (bytes == null) return "";
        const view = ArrayBuffer.isView(bytes)
          ? new Uint8Array(bytes.buffer, bytes.byteOffset, bytes.byteLength)
          : new Uint8Array(bytes);
        return Deno.core.decode(view);
      }
    };
  }

  class URLSearchParams {
    constructor(init = "") {
      this._pairs = [];
      if (typeof init === "string") {
        for (const part of init.replace(/^\?/, "").split("&")) {
          if (!part) continue;
          const [name, value = ""] = part.split("=");
          this._pairs.push([decode(name), decode(value)]);
        }
      } else if (init) {
        for (const [name, value] of Object.entries(init)) this._pairs.push([name, String(value)]);
      }
      function decode(text) {
        return decodeURIComponent(text.replace(/\+/g, " "));
      }
    }
    get(name) {
      return this._pairs.find(([key]) => key === name)?.[1] ?? null;
    }
    getAll(name) {
      return this._pairs.filter(([key]) => key === name).map(([, value]) => value);
    }
    has(name) {
      return this._pairs.some(([key]) => key === name);
    }
    set(name, value) {
      this.delete(name);
      this._pairs.push([name, String(value)]);
    }
    append(name, value) {
      this._pairs.push([name, String(value)]);
    }
    delete(name) {
      this._pairs = this._pairs.filter(([key]) => key !== name);
    }
    forEach(callback) {
      for (const [name, value] of this._pairs) callback(value, name, this);
    }
    entries() {
      return this._pairs[Symbol.iterator]();
    }
    [Symbol.iterator]() {
      return this._pairs[Symbol.iterator]();
    }
    toString() {
      return this._pairs.map(([name, value]) => `${encodeURIComponent(name)}=${encodeURIComponent(value)}`).join("&");
    }
  }

  // Enough of WHATWG URL for web and file addresses.
  class URL {
    constructor(input, base) {
      let text = String(input);
      const absolute = /^[a-zA-Z][a-zA-Z0-9+.-]*:/.test(text);
      if (!absolute) {
        if (base === undefined) throw new TypeError(`Invalid URL: ${text}`);
        const from = new URL(base);
        if (text.startsWith("//")) text = `${from.protocol}${text}`;
        else if (text.startsWith("/")) text = `${from.origin === "null" ? `${from.protocol}//` : from.origin}${text}`;
        else if (text.startsWith("?")) text = `${from.origin}${from.pathname}${text}`;
        else if (text.startsWith("#")) text = `${from.origin}${from.pathname}${from.search}${text}`;
        else {
          const directory = from.pathname.slice(0, from.pathname.lastIndexOf("/") + 1);
          text = `${from.protocol}//${from.host}${path().posix?.normalize?.(directory + text) ?? path().normalize(directory + text)}`;
        }
      }
      const match = /^([a-zA-Z][a-zA-Z0-9+.-]*:)(?:\/\/(?:([^:@/]*)(?::([^@/]*))?@)?([^:/?#]*)(?::(\d+))?)?([^?#]*)(\?[^#]*)?(#.*)?$/.exec(text);
      if (!match) throw new TypeError(`Invalid URL: ${text}`);
      this.protocol = match[1].toLowerCase();
      this.username = match[2] ?? "";
      this.password = match[3] ?? "";
      this.hostname = (match[4] ?? "").toLowerCase();
      this.port = match[5] ?? "";
      this.pathname = match[6] || (this.hostname || this.protocol === "file:" ? "/" : "");
      this.search = match[7] && match[7] !== "?" ? match[7] : "";
      this.hash = match[8] && match[8] !== "#" ? match[8] : "";
      this.searchParams = new URLSearchParams(this.search);
    }
    get host() {
      return this.port ? `${this.hostname}:${this.port}` : this.hostname;
    }
    get origin() {
      return /^(https?|wss?|ftp):$/.test(this.protocol) ? `${this.protocol}//${this.host}` : "null";
    }
    get href() {
      const auth = this.username ? `${this.username}${this.password ? `:${this.password}` : ""}@` : "";
      const slashes = this.hostname || this.protocol === "file:" ? "//" : "";
      return `${this.protocol}${slashes}${auth}${this.host}${this.pathname}${this.search}${this.hash}`;
    }
    toString() {
      return this.href;
    }
    toJSON() {
      return this.href;
    }
  }
  globalThis.URL ??= URL;
  globalThis.URLSearchParams ??= URLSearchParams;

  function fileURLToPath(url) {
    const parsed = typeof url === "string" ? new globalThis.URL(url) : url;
    if (parsed.protocol !== "file:") throw new TypeError("The URL must be of scheme file");
    return decodeURIComponent(parsed.pathname);
  }

  function pathToFileURL(file) {
    const absolute = path().resolve(String(file));
    return new globalThis.URL(`file://${absolute.split("/").map(encodeURIComponent).join("/")}`);
  }

  register("url", () => ({
    URL: globalThis.URL,
    URLSearchParams: globalThis.URLSearchParams,
    fileURLToPath,
    pathToFileURL,
    parse(text) {
      const url = new globalThis.URL(text);
      return {
        protocol: url.protocol,
        auth: url.username ? `${url.username}${url.password ? `:${url.password}` : ""}` : null,
        host: url.host,
        hostname: url.hostname,
        port: url.port || null,
        pathname: url.pathname,
        path: url.pathname + url.search,
        search: url.search || null,
        query: url.search ? url.search.slice(1) : null,
        hash: url.hash || null,
        href: url.href,
      };
    },
    format: (url) => (typeof url === "string" ? url : url.href ?? String(url)),
  }));

  // ---------------------------------------------------------------------
  // util

  function format(first, ...rest) {
    if (typeof first !== "string") return [first, ...rest].map(inspect).join(" ");
    let index = 0;
    const text = first.replace(/%[sdifjoO%]/g, (token) => {
      if (token === "%%") return "%";
      if (index >= rest.length) return token;
      const value = rest[index++];
      switch (token) {
        case "%s":
          return typeof value === "string" ? value : inspect(value);
        case "%d":
        case "%i":
          return String(parseInt(value, 10));
        case "%f":
          return String(parseFloat(value));
        default:
          return inspect(value);
      }
    });
    return [text, ...rest.slice(index).map((value) => (typeof value === "string" ? value : inspect(value)))].join(" ");
  }

  function inspect(value) {
    if (typeof value === "string") return `'${value}'`;
    if (value instanceof Error) return value.stack ?? String(value);
    if (typeof value === "function") return `[Function: ${value.name || "anonymous"}]`;
    try {
      return JSON.stringify(value) ?? String(value);
    } catch {
      return String(value);
    }
  }
  inspect.custom = Symbol.for("nodejs.util.inspect.custom");

  const promisifyCustom = Symbol.for("nodejs.util.promisify.custom");
  function promisify(original) {
    if (original[promisifyCustom]) return original[promisifyCustom];
    return function (...args) {
      return new Promise((resolve, reject) => {
        original.call(this, ...args, (error, ...values) => {
          if (error) reject(error);
          else resolve(values.length > 1 ? values : values[0]);
        });
      });
    };
  }
  promisify.custom = promisifyCustom;

  const util = {
    format,
    inspect,
    promisify,
    callbackify: (original) =>
      function (...args) {
        const callback = args.pop();
        original.apply(this, args).then((value) => callback(null, value), (error) => callback(error));
      },
    inherits(child, parent) {
      Object.setPrototypeOf(child.prototype, parent.prototype);
      Object.setPrototypeOf(child, parent);
      child.super_ = parent;
    },
    deprecate: (fn) => fn,
    debuglog: () => Object.assign(() => {}, { enabled: false }),
    isDeepStrictEqual: (a, b) => JSON.stringify(a) === JSON.stringify(b),
    stripVTControlCharacters: (text) => String(text).replace(/\u001b\[[0-9;]*[A-Za-z]/g, ""),
    styleText: (_style, text) => text,
    types: {
      isPromise: (value) => value instanceof Promise,
      isRegExp: (value) => value instanceof RegExp,
      isDate: (value) => value instanceof Date,
      isNativeError: (value) => value instanceof Error,
      isUint8Array: (value) => value instanceof Uint8Array,
    },
    isArray: Array.isArray,
    TextEncoder: globalThis.TextEncoder,
    TextDecoder: globalThis.TextDecoder,
  };
  register("util", () => util);

  // ---------------------------------------------------------------------
  // os and process

  let osInfo;
  const os = () => (osInfo ??= ops.op_keel_node_os());

  register("os", () => ({
    EOL: "\n",
    platform: () => "darwin",
    type: () => "Darwin",
    arch: () => os().arch,
    release: () => os().release,
    homedir: () => os().homedir,
    tmpdir: () => os().tmpdir,
    hostname: () => os().hostname,
    endianness: () => "LE",
    cpus: () => Array.from({ length: os().cpus }, () => ({ model: "", speed: 0, times: {} })),
    availableParallelism: () => os().cpus,
    totalmem: () => 0,
    freemem: () => 0,
    uptime: () => 0,
    loadavg: () => [0, 0, 0],
    networkInterfaces: () => ({}),
    userInfo: () => ({ username: os().username, homedir: os().homedir, shell: null, uid: -1, gid: -1 }),
    constants: { signals: {}, errno: {} },
    devNull: "/dev/null",
  }));

  const started = Date.now();
  register("process", () => {
    const events = new EventEmitter();
    const stream = (name) => ({
      write: (text) => {
        console.log(String(text).replace(/\n$/, ""));
        return true;
      },
      isTTY: false,
      fd: name === "stdout" ? 1 : 2,
      on: () => {},
      once: () => {},
    });
    const process = Object.create(globalThis.process);
    Object.assign(process, {
      env: Object.assign(Object.fromEntries(os().env), globalThis.process.env),
      arch: os().arch,
      pid: 1,
      ppid: 0,
      execPath: "/keel",
      execArgv: [],
      exitCode: undefined,
      release: { name: "keel" },
      stdout: stream("stdout"),
      stderr: stream("stderr"),
      stdin: { isTTY: false, on: () => {}, fd: 0 },
      nextTick: (callback, ...args) => queueMicrotask(() => callback(...args)),
      hrtime: Object.assign(
        (previous) => {
          const now = performanceNow();
          const seconds = Math.floor(now / 1000);
          const nanos = Math.floor((now % 1000) * 1e6);
          return previous ? [seconds - previous[0], nanos - previous[1]] : [seconds, nanos];
        },
        { bigint: () => BigInt(Math.floor(performanceNow() * 1e6)) },
      ),
      uptime: () => (Date.now() - started) / 1000,
      memoryUsage: () => ({ rss: 0, heapTotal: 0, heapUsed: 0, external: 0 }),
      emitWarning: () => {},
      exit: (code) => {
        throw new Error(`an extension asked to exit Keel (${code ?? 0}); Keel keeps running`);
      },
      on: (...args) => (events.on(...args), process),
      once: (...args) => (events.once(...args), process),
      off: (...args) => (events.off(...args), process),
      removeListener: (...args) => (events.off(...args), process),
      emit: (...args) => events.emit(...args),
      listenerCount: (name) => events.listenerCount(name),
      cwd: () => globalThis.process.cwd(),
      umask: () => 0o022,
      getuid: () => -1,
      getgid: () => -1,
    });
    return process;
  });

  function performanceNow() {
    return globalThis.performance?.now?.() ?? Date.now();
  }

  // ---------------------------------------------------------------------
  // fs

  class Stats {
    constructor(raw) {
      Object.assign(this, {
        dev: raw.dev,
        ino: raw.ino,
        mode: raw.mode,
        nlink: raw.nlink,
        uid: raw.uid,
        gid: raw.gid,
        size: raw.size,
        atimeMs: raw.atimeMs,
        mtimeMs: raw.mtimeMs,
        ctimeMs: raw.ctimeMs,
        birthtimeMs: raw.birthtimeMs,
        atime: new Date(raw.atimeMs),
        mtime: new Date(raw.mtimeMs),
        ctime: new Date(raw.ctimeMs),
        birthtime: new Date(raw.birthtimeMs),
      });
      Object.defineProperty(this, "_kind", { value: raw.kind });
    }
    isFile() {
      return this._kind === 1;
    }
    isDirectory() {
      return this._kind === 2;
    }
    isSymbolicLink() {
      return this._kind === 64;
    }
    isFIFO() {
      return false;
    }
    isSocket() {
      return false;
    }
    isBlockDevice() {
      return false;
    }
    isCharacterDevice() {
      return false;
    }
  }

  class Dirent {
    constructor(name, kind, parent) {
      this.name = name;
      this.parentPath = parent;
      this.path = parent;
      Object.defineProperty(this, "_kind", { value: kind });
    }
  }
  for (const method of ["isFile", "isDirectory", "isSymbolicLink", "isFIFO", "isSocket", "isBlockDevice", "isCharacterDevice"]) {
    Dirent.prototype[method] = Stats.prototype[method];
  }

  const toPath = (file) => (file instanceof globalThis.URL ? fileURLToPath(file) : String(file));
  const encodingOf = (options) => (typeof options === "string" ? options : options?.encoding);
  const toBytes = (data, options) =>
    typeof data === "string" ? Buffer.from(data, encodingOf(options) ?? undefined) : new Uint8Array(data.buffer ?? data, data.byteOffset ?? 0, data.byteLength ?? data.length);

  const sync = {
    readFileSync(file, options) {
      const bytes = attempt(() => ops.op_keel_node_read(toPath(file)));
      const buffer = Buffer.from(bytes);
      const encoding = encodingOf(options);
      return encoding ? buffer.toString(encoding) : buffer;
    },
    writeFileSync(file, data, options) {
      attempt(() => ops.op_keel_node_write(toPath(file), toBytes(data, options), false));
    },
    appendFileSync(file, data, options) {
      attempt(() => ops.op_keel_node_write(toPath(file), toBytes(data, options), true));
    },
    existsSync: (file) => {
      try {
        return ops.op_keel_node_exists(toPath(file));
      } catch {
        return false;
      }
    },
    statSync(file, options) {
      try {
        return new Stats(ops.op_keel_node_stat(toPath(file), true));
      } catch (error) {
        if (options?.throwIfNoEntry === false) return undefined;
        throw nodeError(error);
      }
    },
    lstatSync(file, options) {
      try {
        return new Stats(ops.op_keel_node_stat(toPath(file), false));
      } catch (error) {
        if (options?.throwIfNoEntry === false) return undefined;
        throw nodeError(error);
      }
    },
    readdirSync(file, options) {
      const folder = toPath(file);
      const entries = attempt(() => ops.op_keel_node_readdir(folder));
      return options?.withFileTypes
        ? entries.map(([name, kind]) => new Dirent(name, kind, folder))
        : entries.map(([name]) => name);
    },
    mkdirSync(file, options) {
      const recursive = typeof options === "object" && !!options?.recursive;
      attempt(() => ops.op_keel_node_mkdir(toPath(file), recursive));
      return recursive ? toPath(file) : undefined;
    },
    rmSync(file, options) {
      try {
        ops.op_keel_node_remove(toPath(file), !!options?.recursive, !!options?.recursive);
      } catch (error) {
        if (!(options?.force && nodeError(error).code === "ENOENT")) throw nodeError(error);
      }
    },
    rmdirSync(file, options) {
      attempt(() => ops.op_keel_node_remove(toPath(file), !!options?.recursive, true));
    },
    unlinkSync(file) {
      attempt(() => ops.op_keel_node_remove(toPath(file), false, false));
    },
    renameSync(from, to) {
      attempt(() => ops.op_keel_node_move(toPath(from), toPath(to), false));
    },
    copyFileSync(from, to) {
      attempt(() => ops.op_keel_node_move(toPath(from), toPath(to), true));
    },
    realpathSync(file) {
      return attempt(() => ops.op_keel_node_realpath(toPath(file)));
    },
    accessSync(file) {
      if (!ops.op_keel_node_exists(toPath(file))) {
        const error = new Error(`ENOENT: no such file or directory, access '${toPath(file)}'`);
        error.code = "ENOENT";
        throw error;
      }
    },
  };
  sync.realpathSync.native = sync.realpathSync;

  // The promise and callback forms run the same work.
  const promises = {};
  const callbacks = {};
  for (const [name, work] of Object.entries(sync)) {
    if (name === "existsSync") continue;
    const base = name.slice(0, -4);
    promises[base] = async (...args) => work(...args);
    callbacks[base] = (...args) => {
      const callback = typeof args[args.length - 1] === "function" ? args.pop() : () => {};
      queueMicrotask(() => {
        let result;
        try {
          result = work(...args);
        } catch (error) {
          callback(error);
          return;
        }
        callback(null, result);
      });
    };
  }
  callbacks.realpath.native = callbacks.realpath;
  promises.rm = async (file, options) => sync.rmSync(file, options);
  promises.open = async () => {
    throw new Error("Keel does not open file handles for extensions");
  };

  const constants = { F_OK: 0, R_OK: 4, W_OK: 2, X_OK: 1, O_RDONLY: 0, O_WRONLY: 1, O_RDWR: 2, S_IFMT: 0o170000, S_IFREG: 0o100000, S_IFDIR: 0o040000, S_IFLNK: 0o120000 };
  promises.constants = constants;

  register("fs", () => ({
    ...sync,
    ...callbacks,
    exists: (file, callback) => queueMicrotask(() => callback(sync.existsSync(file))),
    promises,
    constants,
    Stats,
    Dirent,
    watch: () => Object.assign(new EventEmitter(), { close() {} }),
    watchFile: () => {},
    unwatchFile: () => {},
    createReadStream(file, options) {
      const stream = new (keel.builtin("stream").Readable)();
      queueMicrotask(() => {
        try {
          const data = sync.readFileSync(file, options);
          stream.push(data);
          stream.push(null);
        } catch (error) {
          stream.destroy(error);
        }
      });
      return stream;
    },
    createWriteStream(file) {
      const chunks = [];
      const stream = new (keel.builtin("stream").Writable)({
        write(chunk, _encoding, done) {
          chunks.push(typeof chunk === "string" ? Buffer.from(chunk) : chunk);
          done();
        },
        final(done) {
          const size = chunks.reduce((sum, chunk) => sum + chunk.length, 0);
          const all = new Uint8Array(size);
          let at = 0;
          for (const chunk of chunks) {
            all.set(chunk, at);
            at += chunk.length;
          }
          try {
            sync.writeFileSync(file, all);
            done();
          } catch (error) {
            done(error);
          }
        },
      });
      return stream;
    },
  }));
  register("fs/promises", () => promises);

  // ---------------------------------------------------------------------
  // stream: in-memory streams, enough to define classes and pass data

  class Stream extends EventEmitter {
    pipe(destination) {
      this.on("data", (chunk) => destination.write(chunk));
      this.on("end", () => destination.end?.());
      return destination;
    }
  }

  class Readable extends Stream {
    constructor(options = {}) {
      super();
      this._queue = [];
      this._ended = false;
      this.readable = true;
      this.destroyed = false;
      this.readableObjectMode = !!options.objectMode;
      this.readableHighWaterMark = options.highWaterMark ?? 16384;
      if (options.read) this._read = options.read;
    }
    _read() {}
    push(chunk) {
      if (chunk === null) {
        this._ended = true;
        queueMicrotask(() => this.emit("end"));
        this._wake?.();
        return false;
      }
      this._queue.push(chunk);
      queueMicrotask(() => this.emit("data", chunk));
      this._wake?.();
      return true;
    }
    read() {
      return this._queue.shift() ?? null;
    }
    setEncoding() {
      return this;
    }
    resume() {
      return this;
    }
    pause() {
      return this;
    }
    destroy(error) {
      this.destroyed = true;
      if (error) queueMicrotask(() => this.emit("error", error));
      queueMicrotask(() => this.emit("close"));
      this._error = error;
      this._wake?.();
      return this;
    }
    async *[Symbol.asyncIterator]() {
      for (;;) {
        if (this._queue.length) {
          yield this._queue.shift();
          continue;
        }
        if (this._error) throw this._error;
        if (this._ended || this.destroyed) return;
        this._read();
        await new Promise((resolve) => (this._wake = resolve));
      }
    }
    static from(iterable) {
      const stream = new Readable();
      (async () => {
        for await (const chunk of iterable) stream.push(chunk);
        stream.push(null);
      })().catch((error) => stream.destroy(error));
      return stream;
    }
  }

  class Writable extends Stream {
    constructor(options = {}) {
      super();
      this.writable = true;
      this.destroyed = false;
      this.writableObjectMode = !!options.objectMode;
      this.writableHighWaterMark = options.highWaterMark ?? 16384;
      if (options.write) this._write = options.write;
      if (options.final) this._final = options.final;
    }
    _write(_chunk, _encoding, done) {
      done();
    }
    write(chunk, encoding, callback) {
      this._write(chunk, typeof encoding === "string" ? encoding : "utf8", (error) => {
        if (error) this.emit("error", error);
        (typeof encoding === "function" ? encoding : callback)?.(error);
      });
      return true;
    }
    end(chunk, encoding, callback) {
      if (chunk != null && typeof chunk !== "function") this.write(chunk, encoding);
      const done = [chunk, encoding, callback].find((item) => typeof item === "function");
      const finish = (error) => {
        if (error) this.emit("error", error);
        else this.emit("finish");
        this.emit("close");
        done?.(error);
      };
      if (this._final) this._final(finish);
      else queueMicrotask(() => finish());
      return this;
    }
    destroy(error) {
      this.destroyed = true;
      if (error) queueMicrotask(() => this.emit("error", error));
      return this;
    }
    close(callback) {
      this.destroyed = true;
      queueMicrotask(() => {
        this.emit("close");
        callback?.();
      });
    }
  }

  class Duplex extends Readable {
    constructor(options = {}) {
      super(options);
      this.writable = true;
      this.writableObjectMode = !!options.objectMode;
      this.writableHighWaterMark = options.highWaterMark ?? 16384;
      if (options.write) this._write = options.write;
      if (options.final) this._final = options.final;
    }
  }
  for (const method of ["_write", "write", "end"]) {
    Duplex.prototype[method] = Writable.prototype[method];
  }

  class Transform extends Duplex {
    constructor(options = {}) {
      super(options);
      if (options.transform) this._transform = options.transform;
      if (options.flush) this._flush = options.flush;
    }
    _transform(chunk, _encoding, done) {
      done(null, chunk);
    }
    _write(chunk, encoding, done) {
      this._transform(chunk, encoding, (error, output) => {
        if (output != null) this.push(output);
        done(error);
      });
    }
    end(chunk, encoding, callback) {
      if (chunk != null && typeof chunk !== "function") this.write(chunk, encoding);
      const finish = () => {
        this.push(null);
        this.emit("finish");
        [chunk, encoding, callback].find((item) => typeof item === "function")?.();
      };
      if (this._flush) this._flush((error, output) => {
        if (output != null) this.push(output);
        finish();
      });
      else finish();
      return this;
    }
  }

  class PassThrough extends Transform {}

  function finished(stream, callback) {
    const done = (error) => callback?.(error);
    stream.once?.("end", () => done());
    stream.once?.("finish", () => done());
    stream.once?.("error", done);
    return () => {};
  }

  async function pipelineAsync(...streams) {
    let source = streams.shift();
    for (const step of streams) {
      if (typeof step === "function") source = step(source);
      else {
        for await (const chunk of source) step.write(chunk);
        await new Promise((resolve) => step.end(resolve));
        source = step;
      }
    }
    if (source && typeof source.then === "function") await source;
  }

  function pipeline(...streams) {
    const callback = typeof streams[streams.length - 1] === "function" ? streams.pop() : undefined;
    pipelineAsync(...streams).then(() => callback?.(), (error) => callback?.(error));
    return streams[streams.length - 1];
  }

  const streamModule = Object.assign(Stream, {
    Stream,
    Readable,
    Writable,
    Duplex,
    Transform,
    PassThrough,
    finished,
    pipeline,
    getDefaultHighWaterMark: (objectMode) => (objectMode ? 16 : 16384),
    setDefaultHighWaterMark: () => {},
    isReadable: (stream) => !!stream?.readable,
    promises: {
      pipeline: pipelineAsync,
      finished: (stream) => new Promise((resolve, reject) => finished(stream, (error) => (error ? reject(error) : resolve()))),
    },
  });
  register("stream", () => streamModule);
  register("stream/promises", () => streamModule.promises);

  async function collect(stream) {
    const chunks = [];
    for await (const chunk of stream) chunks.push(typeof chunk === "string" ? Buffer.from(chunk) : chunk);
    const size = chunks.reduce((sum, chunk) => sum + chunk.length, 0);
    const all = Buffer.alloc(size);
    let at = 0;
    for (const chunk of chunks) {
      all.set(chunk, at);
      at += chunk.length;
    }
    return all;
  }
  register("stream/consumers", () => ({
    buffer: collect,
    arrayBuffer: async (stream) => (await collect(stream)).buffer,
    text: async (stream) => (await collect(stream)).toString(),
    json: async (stream) => JSON.parse((await collect(stream)).toString()),
  }));

  register("string_decoder", () => ({
    StringDecoder: class StringDecoder {
      write(bytes) {
        return typeof bytes === "string" ? bytes : Deno.core.decode(bytes);
      }
      end(bytes) {
        return bytes ? this.write(bytes) : "";
      }
    },
  }));

  // ---------------------------------------------------------------------
  // http and https: the whole answer, fetched by Keel

  const STATUS_CODES = { 200: "OK", 201: "Created", 204: "No Content", 301: "Moved Permanently", 302: "Found", 304: "Not Modified", 400: "Bad Request", 401: "Unauthorized", 403: "Forbidden", 404: "Not Found", 500: "Internal Server Error" };

  class Agent extends EventEmitter {
    constructor(options = {}) {
      super();
      this.options = options;
      this.keepAlive = !!options.keepAlive;
    }
    destroy() {}
  }

  class ClientRequest extends Writable {
    constructor(url, options, onResponse) {
      super();
      this._url = url;
      this._method = (options.method ?? "GET").toUpperCase();
      this._headers = Object.entries(options.headers ?? {}).map(([name, value]) => [name, String(value)]);
      this._body = [];
      if (onResponse) this.once("response", onResponse);
    }
    setHeader(name, value) {
      this._headers = this._headers.filter(([key]) => key.toLowerCase() !== name.toLowerCase());
      this._headers.push([name, String(value)]);
    }
    getHeader(name) {
      return this._headers.find(([key]) => key.toLowerCase() === name.toLowerCase())?.[1];
    }
    removeHeader(name) {
      this._headers = this._headers.filter(([key]) => key.toLowerCase() !== name.toLowerCase());
    }
    setTimeout() {
      return this;
    }
    setNoDelay() {}
    setSocketKeepAlive() {}
    abort() {
      this.destroyed = true;
    }
    write(chunk) {
      this._body.push(typeof chunk === "string" ? Buffer.from(chunk) : chunk);
      return true;
    }
    end(chunk) {
      if (chunk != null && typeof chunk !== "function") this.write(chunk);
      const size = this._body.reduce((sum, part) => sum + part.length, 0);
      const body = new Uint8Array(size);
      let at = 0;
      for (const part of this._body) {
        body.set(part, at);
        at += part.length;
      }
      ops
        .op_keel_node_http(this._method, this._url, this._headers, body)
        .then((answer) => {
          if (this.destroyed) return;
          const response = new Readable();
          response.statusCode = answer.status;
          response.statusMessage = STATUS_CODES[answer.status] ?? "";
          response.headers = {};
          for (const [name, value] of answer.headers) {
            response.headers[name] = response.headers[name] ? `${response.headers[name]}, ${value}` : value;
          }
          response.rawHeaders = answer.headers.flat();
          this.emit("response", response);
          response.push(Buffer.from(answer.body));
          response.push(null);
        })
        .catch((error) => this.emit("error", nodeError(error)));
      return this;
    }
  }

  function httpModule(protocol) {
    function request(first, second, third) {
      let url;
      let options;
      let callback;
      if (typeof first === "string" || first instanceof globalThis.URL) {
        url = String(first);
        options = typeof second === "object" ? second : {};
        callback = typeof second === "function" ? second : third;
      } else {
        options = first ?? {};
        callback = second;
        const host = options.hostname ?? options.host ?? "localhost";
        const port = options.port ? `:${options.port}` : "";
        url = `${options.protocol ?? protocol}//${host}${port}${options.path ?? "/"}`;
      }
      return new ClientRequest(url, options, callback);
    }
    return {
      request,
      get(...args) {
        const sent = request(...args);
        sent.end();
        return sent;
      },
      Agent,
      globalAgent: new Agent(),
      STATUS_CODES,
      METHODS: ["GET", "HEAD", "POST", "PUT", "DELETE", "PATCH", "OPTIONS"],
    };
  }
  register("http", () => httpModule("http:"));
  register("https", () => httpModule("https:"));

  // ---------------------------------------------------------------------
  // What extensions may load but Keel does not offer

  const notOffered = (what) => () => {
    throw new Error(`Keel does not let extensions use ${what}`);
  };
  const isIPv4 = (text) => /^(\d{1,3}\.){3}\d{1,3}$/.test(String(text));
  const isIPv6 = (text) => String(text).includes(":") && /^[0-9a-fA-F:.]+$/.test(String(text));
  register("net", () => ({
    isIP: (text) => (isIPv4(text) ? 4 : isIPv6(text) ? 6 : 0),
    isIPv4,
    isIPv6,
    Socket: class Socket extends EventEmitter {},
    connect: notOffered("network sockets"),
    createConnection: notOffered("network sockets"),
    createServer: notOffered("servers"),
  }));
  register("tls", () => ({ connect: notOffered("network sockets"), TLSSocket: class TLSSocket extends EventEmitter {} }));
  register("tty", () => ({
    isatty: () => false,
    ReadStream: class ReadStream extends Readable {},
    WriteStream: class WriteStream extends Writable {},
  }));
  register("child_process", () => {
    const refuse = (callback) => {
      const error = Object.assign(new Error("Keel does not let extensions run programs"), { code: "EPERM" });
      if (typeof callback === "function") queueMicrotask(() => callback(error, "", ""));
      else throw error;
      return Object.assign(new EventEmitter(), { kill() {} });
    };
    const execFile = (...args) => refuse(args.find((arg) => typeof arg === "function"));
    execFile[promisifyCustom] = async () => refuse();
    return {
      execFile,
      exec: execFile,
      spawn: () => refuse(),
      fork: () => refuse(),
      execSync: () => refuse(),
      execFileSync: () => refuse(),
      spawnSync: () => refuse(),
    };
  });

  function assert(value, message) {
    if (!value) throw Object.assign(new Error(message ?? "Assertion failed"), { code: "ERR_ASSERTION" });
  }
  Object.assign(assert, {
    ok: assert,
    equal: (a, b, message) => assert(a == b, message ?? `${a} == ${b}`),
    strictEqual: (a, b, message) => assert(a === b, message ?? `${a} === ${b}`),
    notStrictEqual: (a, b, message) => assert(a !== b, message ?? `${a} !== ${b}`),
    deepStrictEqual: (a, b, message) => assert(JSON.stringify(a) === JSON.stringify(b), message ?? "values differ"),
    fail: (message) => assert(false, message),
  });
  assert.strict = assert;
  register("assert", () => assert);
})(globalThis);
