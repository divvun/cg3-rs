//! What the module tells the wrapper unasked: the message of a panic, and the
//! library's tracing events.

use wasm_bindgen::prelude::*;

#[wasm_bindgen(module = "/src/sink.js")]
extern "C" {
    #[wasm_bindgen(js_name = installSink)]
    fn install_sink(sink: JsValue);
    #[wasm_bindgen(js_name = sinkLog)]
    fn sink_log(level: u8, target: &str, message: &str);
    #[wasm_bindgen(js_name = sinkPanic)]
    fn sink_panic(message: &str);
}

// [spec:cg3:req:wasm.errors+1]
/// Starts the module instance: hands it the wrapper's sink, sends every panic
/// message there before the panic aborts into a trap, and sends tracing events
/// at `level` and above there. The wrapper calls it once per module, from that
/// module's `init()`.
#[wasm_bindgen]
pub fn install(sink: JsValue, level: u8) {
    install_sink(sink);
    std::panic::set_hook(Box::new(|info| sink_panic(&info.to_string())));
    crate::log::install(level, sink_log);
}
