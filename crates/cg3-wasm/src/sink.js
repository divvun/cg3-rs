// The wrapper's sink for this module instance, an object with
// log(level, target, message) and panic(message). The panic hook and the
// tracing subscriber must be Send and Sync, so they cannot hold a JavaScript
// object; they call these functions, which hold it here instead. The generated
// glue imports this module once, so the package has one sink.

let sink = null;

export function installSink(value) {
  sink = value;
}

export function sinkLog(level, target, message) {
  if (sink !== null) sink.log(level, target, message);
}

export function sinkPanic(message) {
  if (sink !== null) sink.panic(message);
}
