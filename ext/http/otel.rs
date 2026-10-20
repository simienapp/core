// Copyright 2018-2026 the Simien authors. MIT license.

//! OpenTelemetry is not part of simien_http. Upstream records HTTP server
//! metrics once deno_telemetry is configured, through `OTEL_GLOBALS`. Here
//! [`OtelGlobals`] has no values, so `OTEL_GLOBALS` can never be set and no
//! `OtelInfo` is ever made: the compiler, not a flag, keeps every telemetry
//! branch dead, while the code around it stays as upstream wrote it.

use once_cell::sync::OnceCell;

pub enum OtelGlobals {}

impl OtelGlobals {
  pub fn has_metrics(&self) -> bool {
    match *self {}
  }
}

pub static OTEL_GLOBALS: OnceCell<OtelGlobals> = OnceCell::new();
