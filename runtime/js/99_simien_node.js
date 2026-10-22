// Copyright 2018-2026 the Deno authors. MIT license.

// Simien's bootstrap for isolates granted dangerouslyUseNode (APS 2, option
// A), built from the parts of upstream's 99_main.js that simien_node needs:
// - the Deno namespace (90_deno_ns.js) becomes a hidden global: not
//   enumerable, not writable, not configurable. simien_node reads it as
//   `Deno.*`; every call goes through Simien's permissions (APS 3).
// - Node itself starts on first use, as upstream defers it: the arguments are
//   stashed for node:process and node:module to pick up.
// The embedder calls `globalThis.__simienBootstrapNode(argv0)` once, before
// any app code runs; the function removes itself.

import { core, internals, primordials } from "ext:core/mod.js";
import {
  op_bootstrap_args,
  op_bootstrap_no_color,
  op_bootstrap_pid,
  op_bootstrap_target,
  op_main_module,
  op_ppid,
} from "ext:core/ops";
import { denoNs } from "ext:runtime/90_deno_ns.js";
const os = core.loadExtScript("ext:simien_os/30_os.js");

const {
  ObjectAssign,
  ObjectDefineProperties,
  ObjectDefineProperty,
  ObjectFreeze,
  ObjectGetOwnPropertyDescriptors,
  ReflectOwnKeys,
  Symbol,
  Uint8ArrayPrototype,
  uncurryThis,
} = primordials;

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

// `Deno.test` is a no-op outside a test runner; node:test checks for it.
function noopTest() {}
noopTest.ignore = () => {};
noopTest.only = () => {};
noopTest.beforeAll = () => {};
noopTest.beforeEach = () => {};
noopTest.afterEach = () => {};
noopTest.afterAll = () => {};
noopTest.sanitizer = () => {};
const noopTestEach = () => () => {};
noopTest.each = noopTestEach;
noopTest.only.each = noopTestEach;
noopTest.ignore.each = noopTestEach;

let pid;
let args;

function bootstrapNode(argv0) {
  delete globalThis.__simienBootstrapNode;
  captureHexMethods();
  // `Deno.build` and node:process's arch and platform; set once per isolate.
  if (core.build.target === "unknown") {
    core.setBuildInfo(op_bootstrap_target());
  }

  // Built without spreading denoNs, so its lazy getters stay lazy.
  const finalDenoNs = ObjectDefineProperties(
    {
      internal: internalSymbol,
      [internalSymbol]: internals,
      test: noopTest,
      bench: () => {},
    },
    getSafeOwnPropertyDescriptors(denoNs),
  );
  ObjectDefineProperties(finalDenoNs, {
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

  ObjectDefineProperty(globalThis, "Deno", {
    __proto__: null,
    value: finalDenoNs,
    enumerable: false,
    writable: false,
    configurable: false,
  });

  // node:process and node:module start Node from these on first use.
  internals.__nodeBootstrapArgs = {
    usesLocalNodeModulesDir: true,
    runningOnMainThread: true,
    argv0,
    nodeDebug: "",
    nodeClusterUniqueId: undefined,
    nodeClusterSchedPolicy: undefined,
    denoArgs: finalDenoNs.args,
    denoVersion: finalDenoNs.version,
  };
}

ObjectDefineProperty(globalThis, "__simienBootstrapNode", {
  __proto__: null,
  value: bootstrapNode,
  enumerable: false,
  writable: false,
  configurable: true,
});
