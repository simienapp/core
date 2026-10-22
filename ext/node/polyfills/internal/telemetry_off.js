// Copyright 2018-2026 the Simien authors. MIT license.

// What node:http, its server and node:http2 read from upstream's
// OpenTelemetry extension. Simien has none: tracing and metrics are off, so
// no span is ever started, and nothing here is reached past those checks.
(function () {
const { primordials } = __bootstrap;
const { Error, ObjectFreeze } = primordials;

function unreachable() {
  throw new Error("OpenTelemetry is not part of Simien's Node layer");
}

const otelState = ObjectFreeze({
  TRACING_ENABLED: false,
  METRICS_ENABLED: false,
  PROPAGATORS: ObjectFreeze([]),
});

return {
  otelState,
  builtinTracer: unreachable,
  ContextManager: ObjectFreeze({ active: unreachable }),
  SPAN_KEY: undefined,
  telemetry: ObjectFreeze({
    get meterProvider() {
      return unreachable();
    },
  }),
  updateSpanFromError: unreachable,
};
})();
