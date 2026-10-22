// Copyright 2018-2026 the Deno authors. MIT license.

// Simien: the Deno namespace simien_node reads (APS 2, option A). Cut from
// upstream (APS 2.1): FFI, KV, cron, WebGPU and canvas, telemetry, bundling,
// permission queries, Deno.serveHttp (deprecated; Simien's ext/http has no
// 01_http.js), and the table of unstable APIs.

import { core, internals, primordials } from "ext:core/mod.js";
import {
  op_runtime_cpu_usage,
  op_runtime_memory_usage,
} from "ext:core/ops";

const timers = core.loadExtScript("ext:simien_web/02_timers.js");
const httpClient = core.loadExtScript("ext:simien_fetch/22_http_client.js");
const console = core.loadExtScript("ext:simien_web/01_console.js");
const net = core.loadExtScript("ext:simien_net/01_net.js");
const tls = core.loadExtScript("ext:simien_net/02_tls.js");
// `Deno.serve` is the eager loader chain into 22_body -> 06_streams (the
// 208 KB web streams polyfill). Defer until first access so programs that
// don't use Deno.serve don't pay the parse cost at startup.
let _serveImpl;
function lazyServe() {
  return _serveImpl ??
    (_serveImpl = core.loadExtScript("ext:simien_http/00_serve.js"));
}
// Deno.upgradeWebSocket chains through 23_request -> 22_body -> 06_streams. Defer.
let _websocketImpl;
function lazyWebsocket() {
  return _websocketImpl ??
    (_websocketImpl = core.loadExtScript("ext:simien_http/02_websocket.js"));
}
const errors = core.loadExtScript("ext:runtime/01_errors.js");
const version = core.loadExtScript("ext:runtime/01_version.ts");
const io = core.loadExtScript("ext:simien_io/12_io.js");
const fs = core.loadExtScript("ext:simien_fs/30_fs.js");
const os = core.loadExtScript("ext:simien_os/30_os.js");
const fsEvents = core.loadExtScript("ext:runtime/40_fs_events.js");
// Deno.Command / Deno.run / etc.: 40_process.js extends ReadableStream at
// module body, which pulls 06_streams.js (208 KB). Defer.
let _processImpl;
function lazyProcess() {
  return _processImpl ??
    (_processImpl = core.loadExtScript("ext:simien_process/40_process.js"));
}
const signals = core.loadExtScript("ext:simien_os/40_signals.js");
const tty = core.loadExtScript("ext:runtime/40_tty.js");
const { ObjectDefineProperty, Float64Array } = primordials;

function defineLazyInternal(name, specifier) {
  ObjectDefineProperty(internals, name, {
    __proto__: null,
    configurable: true,
    enumerable: true,
    get() {
      core.loadExtScript(specifier);
      // The script body's `internals.X = value;` ran during loadExtScript;
      // our setter below replaced this accessor with a data property, so
      // this read goes to that data property and returns the real value.
      return internals[name];
    },
    set(value) {
      ObjectDefineProperty(internals, name, {
        __proto__: null,
        value,
        writable: true,
        enumerable: true,
        configurable: true,
      });
    },
  });
}

// `ext:simien_http/00_serve.js` (registers serve internals at module body).
defineLazyInternal("addTrailers", "ext:simien_http/00_serve.js");
defineLazyInternal("serveHttpOnListener", "ext:simien_http/00_serve.js");
defineLazyInternal("serveHttpOnConnection", "ext:simien_http/00_serve.js");
// `ext:simien_http/02_websocket.js`.
defineLazyInternal(
  "buildCaseInsensitiveCommaValueFinder",
  "ext:simien_http/02_websocket.js",
);
// `ext:simien_fetch/23_request.js`.
defineLazyInternal("getCachedAbortSignal", "ext:simien_fetch/23_request.js");
// `ext:simien_process/40_process.js` (registers process internals at module body).
defineLazyInternal("getExtraPipeRids", "ext:simien_process/40_process.js");
defineLazyInternal("getIpcPipeRid", "ext:simien_process/40_process.js");
defineLazyInternal("kExtraStdio", "ext:simien_process/40_process.js");

// the out buffer for `cpuUsage` and `memoryUsage`
const usageBuffer = new Float64Array(4);

const denoNs = {
  Process: undefined,
  run: undefined,
  isatty: tty.isatty,
  writeFileSync: fs.writeFileSync,
  writeFile: fs.writeFile,
  writeTextFileSync: fs.writeTextFileSync,
  writeTextFile: fs.writeTextFile,
  readTextFile: fs.readTextFile,
  readTextFileSync: fs.readTextFileSync,
  readFile: fs.readFile,
  readFileSync: fs.readFileSync,
  watchFs: fsEvents.watchFs,
  chmodSync: fs.chmodSync,
  chmod: fs.chmod,
  chown: fs.chown,
  chownSync: fs.chownSync,
  copyFileSync: fs.copyFileSync,
  cwd: fs.cwd,
  makeTempDirSync: fs.makeTempDirSync,
  makeTempDir: fs.makeTempDir,
  makeTempFileSync: fs.makeTempFileSync,
  makeTempFile: fs.makeTempFile,
  cpuUsage: () => {
    op_runtime_cpu_usage(usageBuffer);
    const { 0: system, 1: user } = usageBuffer;
    return {
      system,
      user,
    };
  },
  memoryUsage: () => {
    op_runtime_memory_usage(usageBuffer);
    const {
      0: rss,
      1: heapTotal,
      2: heapUsed,
      3: external,
    } = usageBuffer;
    return {
      rss,
      heapTotal,
      heapUsed,
      external,
    };
  },
  mkdirSync: fs.mkdirSync,
  mkdir: fs.mkdir,
  chdir: fs.chdir,
  copyFile: fs.copyFile,
  readDirSync: fs.readDirSync,
  readDir: fs.readDir,
  readLinkSync: fs.readLinkSync,
  readLink: fs.readLink,
  realPathSync: fs.realPathSync,
  realPath: fs.realPath,
  removeSync: fs.removeSync,
  remove: fs.remove,
  renameSync: fs.renameSync,
  rename: fs.rename,
  version: version.version,
  build: core.build,
  statSync: fs.statSync,
  lstatSync: fs.lstatSync,
  stat: fs.stat,
  lstat: fs.lstat,
  truncateSync: fs.truncateSync,
  truncate: fs.truncate,
  errors: errors.errors,
  inspect: console.inspect,
  env: os.env,
  exit: os.exit,
  execPath: os.execPath,
  SeekMode: io.SeekMode,
  FsFile: fs.FsFile,
  open: fs.open,
  openSync: fs.openSync,
  create: fs.create,
  createSync: fs.createSync,
  stdin: io.stdin,
  stdout: io.stdout,
  stderr: io.stderr,
  connect: net.connect,
  listen: net.listen,
  loadavg: os.loadavg,
  connectTls: tls.connectTls,
  listenTls: tls.listenTls,
  startTls: tls.startTls,
  symlink: fs.symlink,
  symlinkSync: fs.symlinkSync,
  link: fs.link,
  linkSync: fs.linkSync,
  serve: undefined,
  resolveDns: net.resolveDns,
  upgradeWebSocket: undefined,
  utime: fs.utime,
  utimeSync: fs.utimeSync,
  kill: undefined,
  addSignalListener: signals.addSignalListener,
  removeSignalListener: signals.removeSignalListener,
  refTimer: timers.refTimer,
  unrefTimer: timers.unrefTimer,
  osRelease: os.osRelease,
  osUptime: os.osUptime,
  hostname: os.hostname,
  systemMemoryInfo: os.systemMemoryInfo,
  networkInterfaces: os.networkInterfaces,
  consoleSize: tty.consoleSize,
  gid: os.gid,
  uid: os.uid,
  Command: undefined,
  ChildProcess: undefined,
  spawn: undefined,
  spawnAndWait: undefined,
  spawnAndWaitSync: undefined,
  umask: fs.umask,
  HttpClient: httpClient.HttpClient,
  createHttpClient: httpClient.createHttpClient,
};

core.defineGlobalProperties(denoNs, {
  Process: core.propWritableLazyLoaded(
    (process) => process.Process,
    lazyProcess,
  ),
  run: core.propWritableLazyLoaded((process) => process.run, lazyProcess),
  serve: core.propWritableLazyLoaded((serve) => serve.serve, lazyServe),
  upgradeWebSocket: core.propWritableLazyLoaded(
    (websocket) => websocket.upgradeWebSocket,
    lazyWebsocket,
  ),
  kill: core.propWritableLazyLoaded((process) => process.kill, lazyProcess),
  Command: core.propWritableLazyLoaded(
    (process) => process.Command,
    lazyProcess,
  ),
  ChildProcess: core.propWritableLazyLoaded(
    (process) => process.ChildProcess,
    lazyProcess,
  ),
  spawn: core.propWritableLazyLoaded((process) => process.spawn, lazyProcess),
  spawnAndWait: core.propWritableLazyLoaded(
    (process) => process.spawnAndWait,
    lazyProcess,
  ),
  spawnAndWaitSync: core.propWritableLazyLoaded(
    (process) => process.spawnAndWaitSync,
    lazyProcess,
  ),
});

export { denoNs };
