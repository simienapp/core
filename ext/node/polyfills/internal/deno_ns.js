// Copyright 2018-2026 the Simien authors. MIT license.

// The Deno namespace the Node layer is built on (`Deno.errors`, `Deno.env`,
// `Deno.build`, ...). It is never a global: programs can't see it, and
// libraries checking `typeof Deno` don't think they run on Deno. The
// embedder's bootstrap (simien_node_runtime) defines the namespace's
// properties on this object before any program code runs; every module here
// reads it through `core.loadExtScript`, which returns this same object.
(function () {
const Deno = {};
return { Deno };
})();
