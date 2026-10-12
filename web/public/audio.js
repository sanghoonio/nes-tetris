// The game's sound: chunks of samples posted from the page, played in order. Short of samples,
// it plays silence; more than a tenth of a second ahead, it drops the oldest.
class Samples extends AudioWorkletProcessor {
  constructor() {
    super();
    this.chunks = [];
    this.at = 0; // into the first chunk
    this.queued = 0;
    this.port.onmessage = (e) => {
      this.chunks.push(e.data);
      this.queued += e.data.length;
      const max = sampleRate / 10;
      while (this.queued - this.at > max && this.chunks.length > 1) {
        this.queued -= this.chunks[0].length;
        this.chunks.shift();
        this.at = 0;
      }
    };
  }

  process(_inputs, outputs) {
    const out = outputs[0][0];
    for (let i = 0; i < out.length; i++) {
      const chunk = this.chunks[0];
      if (!chunk) {
        out[i] = 0;
        continue;
      }
      out[i] = chunk[this.at++];
      if (this.at >= chunk.length) {
        this.chunks.shift();
        this.queued -= chunk.length;
        this.at = 0;
      }
    }
    for (let c = 1; c < outputs[0].length; c++) outputs[0][c].set(out);
    return true;
  }
}

registerProcessor("samples", Samples);
