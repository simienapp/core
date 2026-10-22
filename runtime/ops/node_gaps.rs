// Copyright 2018-2026 the Deno authors. MIT license.

//! Ops simien_node's modules import that live outside the crates Simien
//! forked. They exist so every module loads; what Simien doesn't support yet
//! says so when used:
//! - `op_current_thread_cpu_usage` (node:process), from upstream's
//!   runtime/ops/worker_host.rs, as it is;
//! - node:worker_threads: not supported yet (needs Simien's own workers);
//! - node:sqlite's backup: not supported yet (no simien_node_sqlite);
//! - Deno's serve hooks (internal/http/address_override.js): Simien has no
//!   `deno serve`, so nothing overrides the address and nobody waits;
//! - node:test snapshots: never in update mode.

use simien_core::op2;
use simien_error::JsErrorBox;

simien_core::extension!(
  simien_node_gaps,
  ops = [
    op_current_thread_cpu_usage,
    op_create_worker,
    op_host_get_worker_cpu_usage,
    op_host_post_message,
    op_host_post_message_raw,
    op_host_recv_ctrl,
    op_host_recv_message,
    op_host_recv_message_sync,
    op_host_terminate_worker,
    op_node_worker_thread_post_message,
    op_node_worker_thread_recv_message,
    op_node_worker_thread_register,
    op_node_worker_thread_set_listener_count,
    op_node_database_backup,
    op_http_notify_serving,
    op_http_serve_address_override,
    op_test_snapshot_in_update_mode,
  ],
);

fn not_supported(what: &str) -> JsErrorBox {
  JsErrorBox::new(
    "NotSupported",
    format!("{what} isn't supported in Simien yet (simienapp/runtime#46)"),
  )
}

#[op2(fast)]
fn op_create_worker() -> Result<(), JsErrorBox> {
  Err(not_supported("node:worker_threads"))
}

#[op2(fast)]
fn op_host_get_worker_cpu_usage() -> Result<(), JsErrorBox> {
  Err(not_supported("node:worker_threads"))
}

#[op2(fast)]
fn op_host_post_message() -> Result<(), JsErrorBox> {
  Err(not_supported("node:worker_threads"))
}

#[op2(fast)]
fn op_host_post_message_raw() -> Result<(), JsErrorBox> {
  Err(not_supported("node:worker_threads"))
}

#[op2(fast)]
fn op_host_recv_ctrl() -> Result<(), JsErrorBox> {
  Err(not_supported("node:worker_threads"))
}

#[op2(fast)]
fn op_host_recv_message() -> Result<(), JsErrorBox> {
  Err(not_supported("node:worker_threads"))
}

#[op2(fast)]
fn op_host_recv_message_sync() -> Result<(), JsErrorBox> {
  Err(not_supported("node:worker_threads"))
}

#[op2(fast)]
fn op_host_terminate_worker() -> Result<(), JsErrorBox> {
  Err(not_supported("node:worker_threads"))
}

#[op2(fast)]
fn op_node_worker_thread_post_message() -> Result<(), JsErrorBox> {
  Err(not_supported("node:worker_threads"))
}

#[op2(fast)]
fn op_node_worker_thread_recv_message() -> Result<(), JsErrorBox> {
  Err(not_supported("node:worker_threads"))
}

#[op2(fast)]
fn op_node_worker_thread_register() -> Result<(), JsErrorBox> {
  Err(not_supported("node:worker_threads"))
}

#[op2(fast)]
fn op_node_worker_thread_set_listener_count() -> Result<(), JsErrorBox> {
  Err(not_supported("node:worker_threads"))
}

#[op2(fast)]
fn op_node_database_backup() -> Result<(), JsErrorBox> {
  Err(not_supported("node:sqlite's backup()"))
}

/// Upstream wakes `deno serve`'s waiter; Simien has no `deno serve`.
#[op2(fast)]
fn op_http_notify_serving(#[smi] _kind: u32) {}

/// Upstream reads `DENO_SERVE_ADDRESS` or a tunnel. Simien reads no
/// environment for product behaviour and has no tunnels: no override.
#[op2]
#[serde]
fn op_http_serve_address_override() -> (u8, String, u32, bool) {
  (0, String::new(), 0, false)
}

#[op2(fast)]
fn op_test_snapshot_in_update_mode() -> bool {
  false
}

#[op2(fast)]
fn op_current_thread_cpu_usage(#[buffer] out: &mut [f64]) {
  if out.len() < 2 {
    return;
  }

  let handle = capture_current_thread_handle();
  let (user, system) = get_thread_cpu_usage_by_handle(handle);
  out[0] = user;
  out[1] = system;
}

#[cfg(target_os = "macos")]
fn capture_current_thread_handle() -> u64 {
  // SAFETY: FFI call to get the current thread's Mach port.
  unsafe { mach_thread_self() as u64 }
}

#[cfg(target_os = "macos")]
fn get_thread_cpu_usage_by_handle(handle: u64) -> (f64, f64) {
  let thread_port = handle as u32;
  // SAFETY: thread_info() will initialize this
  let mut info: ThreadBasicInfo = unsafe { std::mem::zeroed() };
  let mut count: u32 = THREAD_BASIC_INFO_COUNT;

  // SAFETY: FFI call to query thread CPU times
  let kr = unsafe {
    thread_info(
      thread_port,
      THREAD_BASIC_INFO,
      (&raw mut info) as *mut i32,
      &mut count,
    )
  };

  if kr != 0 {
    return (0.0, 0.0);
  }

  let user =
    info.user_time_seconds as f64 * 1e6 + info.user_time_microseconds as f64;
  let system = info.system_time_seconds as f64 * 1e6
    + info.system_time_microseconds as f64;
  (user, system)
}

#[cfg(target_os = "macos")]
const THREAD_BASIC_INFO: u32 = 3;
#[cfg(target_os = "macos")]
const THREAD_BASIC_INFO_COUNT: u32 = 10;

#[cfg(target_os = "macos")]
#[repr(C)]
struct ThreadBasicInfo {
  user_time_seconds: i32,
  user_time_microseconds: i32,
  system_time_seconds: i32,
  system_time_microseconds: i32,
  cpu_usage: i32,
  policy: i32,
  run_state: i32,
  flags: i32,
  suspend_count: i32,
  sleep_time: i32,
}

#[cfg(target_os = "macos")]
unsafe extern "C" {
  fn mach_thread_self() -> u32;
  fn thread_info(
    target_act: u32,
    flavor: u32,
    thread_info_out: *mut i32,
    thread_info_outCnt: *mut u32,
  ) -> i32;
}

#[cfg(target_os = "linux")]
fn capture_current_thread_handle() -> u64 {
  // SAFETY: syscall to get the current thread ID.
  unsafe { libc::syscall(libc::SYS_gettid) as u64 }
}

#[cfg(target_os = "linux")]
fn get_thread_cpu_usage_by_handle(handle: u64) -> (f64, f64) {
  let tid = handle as i32;
  let path = format!("/proc/self/task/{}/stat", tid);
  #[allow(clippy::disallowed_methods, reason = "requires real fs")]
  if let Ok(contents) = std::fs::read_to_string(&path) {
    // Parse utime and stime after pid(comm)
    if let Some(pos) = contents.rfind(')') {
      let rest = &contents[pos + 2..]; // skip ") "
      let fields: Vec<&str> = rest.split_whitespace().collect();
      // 0=state 1=ppid ... 11=utime 12=stime
      if fields.len() > 12 {
        // SAFETY: sysconf call to get clock ticks per second.
        let ticks_per_sec = unsafe { libc::sysconf(libc::_SC_CLK_TCK) } as f64;
        let utime = fields[11].parse::<f64>().unwrap_or(0.0);
        let stime = fields[12].parse::<f64>().unwrap_or(0.0);
        let user_us = utime / ticks_per_sec * 1e6;
        let system_us = stime / ticks_per_sec * 1e6;
        return (user_us, system_us);
      }
    }
  }
  (0.0, 0.0)
}

#[cfg(windows)]
fn capture_current_thread_handle() -> u64 {
  // SAFETY: Returns the thread ID of the calling thread.
  unsafe { windows_sys::Win32::System::Threading::GetCurrentThreadId() as u64 }
}

#[cfg(windows)]
fn get_thread_cpu_usage_by_handle(handle: u64) -> (f64, f64) {
  use windows_sys::Win32::Foundation::CloseHandle;
  use windows_sys::Win32::Foundation::FALSE;
  use windows_sys::Win32::Foundation::FILETIME;
  use windows_sys::Win32::System::Threading::GetThreadTimes;
  use windows_sys::Win32::System::Threading::OpenThread;
  use windows_sys::Win32::System::Threading::THREAD_QUERY_INFORMATION;

  let thread_id = handle as u32;

  // SAFETY: Opens a handle to the thread for querying times.
  let thread_handle =
    unsafe { OpenThread(THREAD_QUERY_INFORMATION, FALSE, thread_id) };
  if thread_handle.is_null() {
    return (0.0, 0.0);
  }

  let mut creation_time = std::mem::MaybeUninit::<FILETIME>::uninit();
  let mut exit_time = std::mem::MaybeUninit::<FILETIME>::uninit();
  let mut kernel_time = std::mem::MaybeUninit::<FILETIME>::uninit();
  let mut user_time = std::mem::MaybeUninit::<FILETIME>::uninit();

  // SAFETY: Queries thread CPU times.
  let ret = unsafe {
    GetThreadTimes(
      thread_handle,
      creation_time.as_mut_ptr(),
      exit_time.as_mut_ptr(),
      kernel_time.as_mut_ptr(),
      user_time.as_mut_ptr(),
    )
  };

  // SAFETY: Close the thread handle.
  unsafe { CloseHandle(thread_handle) };

  if ret == FALSE {
    return (0.0, 0.0);
  }

  // SAFETY: values are initialized.
  let user_time = unsafe { user_time.assume_init() };
  // SAFETY: values are initialized.
  let kernel_time = unsafe { kernel_time.assume_init() };

  // FILETIME is in 100-nanosecond intervals, convert to microseconds.
  let user_us = ((user_time.dwHighDateTime as u64) << 32
    | user_time.dwLowDateTime as u64) as f64
    / 10.0;
  let system_us = ((kernel_time.dwHighDateTime as u64) << 32
    | kernel_time.dwLowDateTime as u64) as f64
    / 10.0;
  (user_us, system_us)
}

#[cfg(not(any(target_os = "macos", target_os = "linux", windows)))]
fn capture_current_thread_handle() -> u64 {
  0
}

#[cfg(not(any(target_os = "macos", target_os = "linux", windows)))]
fn get_thread_cpu_usage_by_handle(_handle: u64) -> (f64, f64) {
  (0.0, 0.0)
}
