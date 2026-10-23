// Copyright 2018-2026 the Deno authors. MIT license.

// Simien's bootstrap for isolates granted dangerouslyUseNode (APS 2, option
// A), built from the parts of upstream's 99_main.js that simien_node needs:
// - the Deno namespace (90_deno_ns.js) fills simien_node's private
//   `internal/deno_ns.js` object, which simien_node reads as `Deno.*`; every
//   call goes through Simien's permissions (APS 3). It is never a global:
//   Simien doesn't present itself as Deno, and core's bootstrap `Deno`
//   (`Deno.core`) is removed from the global too.
// - Node itself starts on first use, as upstream defers it: the arguments are
//   stashed for node:process and node:module to pick up.
// The embedder calls `globalThis.__simienBootstrapNode(argv0, versions)` once, before
// any app code runs; the function removes itself and returns the lifecycle
// hooks the embedder runs the program with.

import { core, internals, primordials } from "ext:core/mod.js";
import {
  op_bootstrap_args,
  op_bootstrap_cluster,
  op_bootstrap_no_color,
  op_bootstrap_pid,
  op_bootstrap_stderr_no_color,
  op_bootstrap_stdout_no_color,
  op_bootstrap_target,
  op_main_module,
  op_node_has_child_ipc_pipe,
  op_ppid,
} from "ext:core/ops";
import { denoNs } from "ext:runtime/90_deno_ns.js";
const { errors } = core.loadExtScript("ext:runtime/01_errors.js");
const os = core.loadExtScript("ext:simien_os/30_os.js");
const { Deno } = core.loadExtScript("ext:simien_node/internal/deno_ns.js");

const {
  ArrayPrototypePush,
  ArrayPrototypeShift,
  ArrayPrototypeSome,
  ObjectAssign,
  ObjectDefineProperties,
  ObjectDefineProperty,
  ObjectFreeze,
  ObjectGetOwnPropertyDescriptors,
  ObjectKeys,
  ObjectSetPrototypeOf,
  PromisePrototypeThen,
  ReflectApply,
  ReflectOwnKeys,
  Symbol,
  Uint8ArrayPrototype,
  uncurryThis,
} = primordials;

// Node's own modules behind its globals, loaded on first use (as upstream).
const lazyProcess = core.createLazyLoader("node:process");
const lazyBuffer = core.createLazyLoader("node:buffer");
const lazyTimers = core.createLazyLoader("node:timers");
const event = core.loadExtScript("ext:simien_web/02_event.js");
const lazyGlobalScope = core.createLazyLoader(
  "ext:runtime/98_global_scope_shared.js",
);
const webidl = core.loadExtScript("ext:simien_webidl/00_webidl.js");

// The global is an EventTarget, as upstream makes it: node:process maps its
// "exit", "beforeExit" and rejection listeners onto global events.
// Ops fail with these classes (NotFound, AlreadyExists, ...); registered so
// they arrive as `Deno.errors.*`, which node:fs and the rest map to Node's
// error codes. As upstream registers them.
function registerErrorClasses() {
  for (const name of ObjectKeys(errors)) {
    // simien_core registers these itself.
    if (name === "BadResource" || name === "Interrupted" || name === "NotCapable") {
      continue;
    }
    core.registerErrorClass(name, errors[name]);
  }
}

function makeGlobalAnEventTarget() {
  if (typeof globalThis.addEventListener !== "function") {
    ObjectSetPrototypeOf(globalThis, event.EventTargetPrototype);
  }
  globalThis[webidl.brand] = webidl.brand;
  event.setEventTargetData(globalThis);
  event.saveGlobalThisReference(globalThis);
  event.defineEventHandler(globalThis, "unhandledrejection");
  core.setReportExceptionCallback(event.reportException);
  core.setUnhandledPromiseRejectionHandler(processUnhandledPromiseRejection);
  core.setHandledPromiseRejectionHandler(processRejectionHandled);
}

// A rejection nothing handled: "unhandledrejection" listeners first, then
// node:process (which raises it as Node does). true when it was handled.
function processUnhandledPromiseRejection(promise, reason) {
  const rejectionEvent = new event.PromiseRejectionEvent(
    "unhandledrejection",
    { cancelable: true, promise, reason },
  );
  globalThis.dispatchEvent(rejectionEvent);
  if (
    !rejectionEvent.defaultPrevented &&
    typeof internals.nodeProcessUnhandledRejectionCallback !== "undefined"
  ) {
    internals.nodeProcessUnhandledRejectionCallback(rejectionEvent);
  }
  return rejectionEvent.defaultPrevented;
}

function processRejectionHandled(promise, reason) {
  const rejectionHandledEvent = new event.PromiseRejectionEvent(
    "rejectionhandled",
    { promise, reason },
  );
  globalThis.dispatchEvent(rejectionHandledEvent);
  if (typeof internals.nodeProcessRejectionHandledCallback !== "undefined") {
    internals.nodeProcessRejectionHandledCallback(rejectionHandledEvent);
  }
}

// What the embedder calls around the program, as Node runs one (see
// simien_node_runtime::Lifecycle). node:process sets its handlers when it
// starts, so they're looked up at call time; before then nothing listens.
const lifecycle = {
  __proto__: null,
  load() {
    globalThis.dispatchEvent(new event.Event("load"));
  },
  beforeUnload() {
    return globalThis.dispatchEvent(
      new event.Event("beforeunload", { cancelable: true }),
    );
  },
  unload() {
    globalThis.dispatchEvent(new event.Event("unload"));
  },
  processBeforeExit() {
    return internals.dispatchProcessBeforeExitEvent
      ? ReflectApply(internals.dispatchProcessBeforeExitEvent, internals, [])
      : false;
  },
  processExit() {
    if (internals.dispatchProcessExitEvent) {
      ReflectApply(internals.dispatchProcessExitEvent, internals, []);
    }
  },
};

function getSafeOwnPropertyDescriptors(object) {
  const descriptors = ObjectGetOwnPropertyDescriptors(object);
  const safeDescriptors = { __proto__: null };
  const keys = ReflectOwnKeys(descriptors);
  for (let i = 0; i < keys.length; i++) {
    const key = keys[i];
    safeDescriptors[key] = { __proto__: null, ...descriptors[key] };
  }
  return safeDescriptors;
}

// The TC39 arraybuffer-base64 methods for ext:simien_node/internal/buffer.mjs,
// captured before any app code runs (as upstream does).
function captureHexMethods() {
  const toHex = Uint8ArrayPrototype.toHex;
  const setFromHex = Uint8ArrayPrototype.setFromHex;
  internals.uint8ArrayToHex = typeof toHex === "function"
    ? uncurryThis(toHex)
    : undefined;
  internals.uint8ArraySetFromHex = typeof setFromHex === "function"
    ? uncurryThis(setFromHex)
    : undefined;
}

// `Deno[Deno.internal]` keeps upstream's internal surface; extension loading
// stays on the core object imported through `ext:core/mod.js`.
const userVisibleCoreDescriptors = getSafeOwnPropertyDescriptors(core);
delete userVisibleCoreDescriptors.createLazyLoader;
delete userVisibleCoreDescriptors.loadExtScript;
const userVisibleCore = ObjectFreeze(ObjectDefineProperties(
  { __proto__: null },
  userVisibleCoreDescriptors,
));
ObjectAssign(internals, { core: userVisibleCore });
const internalSymbol = Symbol("Deno.internal");

// node:test registers its tests through `Deno.test(options)` and runs
// subtests with `context.step(options)`. Upstream only runs them under
// `deno test`; here they run as Node runs them: one after another once the
// program's own code has run, a failure printed and the exit code set to 1.
const registeredTests = [];
let testsScheduled = false;

function registerTest(options) {
  ArrayPrototypePush(registeredTests, options);
  if (!testsScheduled) {
    testsScheduled = true;
    lazyTimers().setImmediate(runRegisteredTests);
  }
}

async function runRegisteredTests() {
  const only = ArrayPrototypeSome(registeredTests, (test) => test.only);
  let failed = false;
  while (registeredTests.length > 0) {
    const test = ArrayPrototypeShift(registeredTests);
    if (test.ignore || (only && !test.only)) continue;
    if (!(await runTest(test.name, test.fn, ""))) failed = true;
  }
  testsScheduled = false;
  if (failed) globalThis.process.exitCode = 1;
}

// Runs one test or step; whether it and all its steps passed.
async function runTest(name, fn, indent) {
  let stepFailed = false;
  const context = {
    name,
    step(nameOrOptions, stepFn) {
      const options = typeof nameOrOptions === "string"
        ? { name: nameOrOptions, fn: stepFn }
        : nameOrOptions;
      return PromisePrototypeThen(
        runTest(options.name, options.fn, indent + "  "),
        (ok) => {
          if (!ok) stepFailed = true;
          return ok;
        },
      );
    },
  };
  try {
    await fn(context);
  } catch (error) {
    globalThis.console.log(`${indent}\u2716 ${name}`);
    globalThis.console.error(error);
    return false;
  }
  if (stepFailed) {
    globalThis.console.log(`${indent}\u2716 ${name}`);
    return false;
  }
  globalThis.console.log(`${indent}\u2714 ${name}`);
  return true;
}

registerTest.ignore = (options) =>
  registerTest(ObjectAssign({}, options, { ignore: true }));
registerTest.only = (options) =>
  registerTest(ObjectAssign({}, options, { only: true }));

let pid;
let args;

// `versions` is `{ simien, v8 }`: what `process.versions` adds to Node's.
function bootstrapNode(argv0, versions) {
  delete globalThis.__simienBootstrapNode;
  captureHexMethods();
  // `Deno.build` and node:process's arch and platform; set once per isolate.
  if (core.build.target === "unknown") {
    core.setBuildInfo(op_bootstrap_target());
  }
  // console colors a stream only when it's a terminal (and color is on), as
  // upstream's 99_main.js sets it: output piped to another program is plain.
  const { setNoColorFns } = core.loadExtScript("ext:simien_web/01_console.js");
  setNoColorFns(
    () => op_bootstrap_stdout_no_color(),
    () => op_bootstrap_stderr_no_color(),
  );

  // Built without spreading denoNs, so its lazy getters stay lazy.
  ObjectDefineProperties(Deno, {
    internal: core.propReadOnly(internalSymbol),
    [internalSymbol]: core.propReadOnly(internals),
    test: core.propReadOnly(registerTest),
    bench: core.propReadOnly(() => {}),
  });
  ObjectDefineProperties(Deno, getSafeOwnPropertyDescriptors(denoNs));
  ObjectDefineProperties(Deno, {
    version: core.propReadOnly(ObjectFreeze({
      __proto__: null,
      simien: versions.simien,
      v8: versions.v8,
    })),
    pid: core.propGetterOnly(() => pid ??= op_bootstrap_pid()),
    // `ppid` should not be memoized.
    ppid: core.propGetterOnly(() => op_ppid()),
    noColor: core.propGetterOnly(() => op_bootstrap_no_color()),
    args: core.propGetterOnly(() => args ??= op_bootstrap_args()),
    mainModule: core.propGetterOnly(() => op_main_module()),
    exitCode: {
      __proto__: null,
      get() {
        return os.getExitCode();
      },
      set(value) {
        os.setExitCode(value);
      },
    },
  });
  delete globalThis.Deno;

  // Node's globals: only isolates granted dangerouslyUseNode get them.
  // `console` is node:console's (the same object), not the embedder's.
  ObjectDefineProperties(globalThis, {
    console: lazyGlobalScope().windowOrWorkerGlobalScope.console,
    global: core.propWritable(globalThis),
    process: core.propWritableLazyLoaded((m) => m.default, lazyProcess),
    Buffer: core.propWritableLazyLoaded((m) => m.Buffer, lazyBuffer),
    // node:timers' timers return Timeout objects (ref, unref, refresh).
    setTimeout: core.propWritableLazyLoaded((m) => m.setTimeout, lazyTimers),
    setInterval: core.propWritableLazyLoaded((m) => m.setInterval, lazyTimers),
    clearTimeout: core.propWritableLazyLoaded((m) => m.clearTimeout, lazyTimers),
    clearInterval: core.propWritableLazyLoaded(
      (m) => m.clearInterval,
      lazyTimers,
    ),
    setImmediate: core.propWritableLazyLoaded((m) => m.setImmediate, lazyTimers),
    clearImmediate: core.propWritableLazyLoaded(
      (m) => m.clearImmediate,
      lazyTimers,
    ),
  });

  // node:process and node:module start Node from these on first use.
  const { 0: clusterUniqueId, 1: clusterSchedPolicy } = op_bootstrap_cluster();
  internals.__nodeBootstrapArgs = {
    usesLocalNodeModulesDir: true,
    runningOnMainThread: true,
    argv0,
    nodeDebug: "",
    nodeClusterUniqueId: clusterUniqueId ?? undefined,
    nodeClusterSchedPolicy: clusterSchedPolicy ?? undefined,
    denoArgs: Deno.args,
    denoVersion: Deno.version,
  };

  registerErrorClasses();
  makeGlobalAnEventTarget();
  // A child started with fork(): its channel to the parent (process.send,
  // process.on("message")). Upstream connects it in node:module's
  // initialize(), which deferred startup never runs, so it's done here.
  if (op_node_has_child_ipc_pipe()) {
    lazyProcess();
    core.loadExtScript("ext:simien_node/child_process.ts");
    internals.__setupChildProcessIpcChannel();
  }
  // A `cluster` worker: the same, from initialize() upstream.
  if (clusterUniqueId) {
    core.loadExtScript("ext:simien_node/cluster.ts");
    internals.__initCluster(clusterUniqueId, clusterSchedPolicy);
  }
  return lifecycle;
}

ObjectDefineProperty(globalThis, "__simienBootstrapNode", {
  __proto__: null,
  value: bootstrapNode,
  enumerable: false,
  writable: false,
  configurable: true,
});
