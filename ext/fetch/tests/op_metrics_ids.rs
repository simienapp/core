// Copyright 2018-2026 the Deno authors. MIT license.

//! The op metrics factory is told how many ops there are, and every op id it
//! sees is below that number. simien_web declares native objects whose
//! methods are ops of their own, numbered before the regular ops; 0.1.0
//! counted one per object instead of one per method, so an embedder sizing a
//! table by the total indexed past its end.

use std::cell::RefCell;
use std::rc::Rc;

use simien_core::JsRuntime;
use simien_core::RuntimeOptions;

#[test]
fn every_op_id_is_below_the_total() {
  let seen = Rc::new(RefCell::new(Vec::new()));
  let record = seen.clone();
  let _runtime = JsRuntime::new(RuntimeOptions {
    extensions: vec![
      simien_webidl::simien_webidl::init(),
      simien_web::simien_web::init(simien_web::BlobStore::default_arc(), None),
      simien_fetch::simien_fetch::init(Default::default()),
    ],
    op_metrics_factory_fn: Some(Box::new(move |id, total, _| {
      record.borrow_mut().push((id, total));
      None
    })),
    ..Default::default()
  });
  let seen = seen.borrow();
  assert!(!seen.is_empty());
  for &(id, total) in seen.iter() {
    assert!((id as usize) < total, "op id {id} is not below the total {total}");
  }
}
