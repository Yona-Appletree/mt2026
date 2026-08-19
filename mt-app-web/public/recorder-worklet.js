// The only JavaScript in mt2026, and it stays this small on purpose: an
// AudioWorkletProcessor that copies each 128-frame block of the first input
// channel and posts it to the main thread. No resampling, no gating, no
// analysis, no buffering policy — every decision of that kind lives in Rust
// (see `src/recorder.rs`).
class RecorderProcessor extends AudioWorkletProcessor {
  process(inputs) {
    const channel = inputs[0] && inputs[0][0];
    if (channel && channel.length > 0) {
      // The render quantum's buffer is reused between calls, so the copy is
      // mandatory; transferring it hands ownership over without a second one.
      const block = new Float32Array(channel);
      this.port.postMessage(block, [block.buffer]);
    }
    // Keep the processor alive for as long as the node is connected; the
    // main thread decides when recording ends.
    return true;
  }
}

registerProcessor("recorder-processor", RecorderProcessor);
