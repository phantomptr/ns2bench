// Plays 48 kHz stereo s16le chunks pushed from the page, through a small jitter buffer.
class PcmPlayer extends AudioWorkletProcessor {
  constructor() {
    super();
    this.queue = [];          // Float32Array frames, interleaved L/R
    this.offset = 0;
    this.buffered = 0;        // samples per channel waiting
    this.started = false;
    this.port.onmessage = (e) => {
      const i16 = new Int16Array(e.data);
      const f = new Float32Array(i16.length);
      for (let i = 0; i < i16.length; i++) f[i] = i16[i] / 32768;
      this.queue.push(f);
      this.buffered += f.length / 2;
      // keep latency bounded: drop the oldest audio beyond ~300 ms
      while (this.buffered > sampleRate * 0.3 && this.queue.length > 1) {
        const old = this.queue.shift();
        this.buffered -= (old.length - this.offset) / 2;
        this.offset = 0;
      }
    };
  }
  process(_, outputs) {
    const [l, r] = outputs[0];
    if (!this.started && this.buffered < sampleRate * 0.08) return true;   // ~80 ms cushion
    this.started = true;
    for (let i = 0; i < l.length; i++) {
      const cur = this.queue[0];
      if (!cur) { l[i] = 0; if (r) r[i] = 0; this.started = this.buffered > 0; continue; }
      l[i] = cur[this.offset];
      if (r) r[i] = cur[this.offset + 1];
      this.offset += 2;
      this.buffered -= 1;
      if (this.offset >= cur.length) { this.queue.shift(); this.offset = 0; }
    }
    return true;
  }
}
registerProcessor('pcm-player', PcmPlayer);
