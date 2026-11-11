// The wrapper's sink for this module instance, an object with
// log(level, target, message) and panic(message). The panic hook and the
// tracing subscriber must be Send and Sync, so they cannot hold a JavaScript
// object; they call these functions, which hold it here instead. Each module's
// generated glue imports its own copy of this file, from the snippets beside
// it, so each module has one sink. Were the two glues to share one copy, the
// second module's install would take the first module's sink.

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
