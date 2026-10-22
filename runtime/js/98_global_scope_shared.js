// Copyright 2018-2026 the Deno authors. MIT license.

// Simien: upstream assembles every global of a window or worker here. Simien's
// runtime installs its own globals, so this keeps only what simien_node reads:
// the console that `node:console` (console_esm.ts) re-exports, built as
// upstream builds it. The rest is cut (APS 2.1).

import { core } from "ext:core/mod.js";

const console = core.loadExtScript("ext:simien_web/01_console.js");

const windowOrWorkerGlobalScope = {
  console: core.propNonEnumerable(
    new console.Console((msg, level) => core.print(msg, level > 1)),
  ),
};

export { windowOrWorkerGlobalScope };
