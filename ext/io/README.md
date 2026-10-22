# simien_io

**This crate provides IO primitives for other Deno extensions, this includes
stdio streams and abstraction over File System files.**

## Usage Example

From javascript, include the extension's source:

```javascript
import { core } from "ext:core/mod.js";

const io = core.loadExtScript("ext:simien_io/12_io.js");
```

Then from rust, provide: `simien_io::simien_io::init(Option<simien_io::Stdio>)` in the
`extensions` field of your `RuntimeOptions`

Where `simien_io::Stdio` implements `Default`, and can therefore be provided as
`Some(simien_io::Stdio::default())`

## Dependencies

- **simien_web**: Provided by the `simien_web` crate
- **deno_tty**: Provided in `deno/runtime/ops/tty.rs`
