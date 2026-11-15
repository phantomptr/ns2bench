import { authURL } from './client.js';
// Live view: hardware H.264 over the /video WebSocket, decoded with WebCodecs onto a canvas.
// Falls back to the MJPEG <img> when the server, browser or codec can't do it.

export function startLive(img, onMode = () => {}) {
  const canvas = document.createElement('canvas');
  canvas.className = 'live';
  canvas.hidden = true;
  img.after(canvas);
  const ctx = canvas.getContext('2d', { alpha: false, desynchronized: true });
  let ws = null, dec = null, mode = 'mjpeg', failures = 0, ts = 0, latest = null, needKey = false, retry = 0;

  const mjpeg = (why) => {
    if (mode !== 'mjpeg') console.info('[live] MJPEG fallback:', why);
    mode = 'mjpeg';
    canvas.hidden = true; img.hidden = false;
    if (!img.src.includes('stream.mjpg')) img.src = authURL(`/stream.mjpg?r=${Date.now()}`);
    onMode(mode);
  };

  const paint = () => {
    if (latest) {
      if (canvas.width !== latest.displayWidth) { canvas.width = latest.displayWidth; canvas.height = latest.displayHeight; }
      ctx.drawImage(latest, 0, 0);
      latest.close(); latest = null;
    }
    if (mode === 'h264') requestAnimationFrame(paint);
  };

  async function configure(init) {
    if (!('VideoDecoder' in window)) throw new Error('no WebCodecs');
    const config = { codec: init.codec, optimizeForLatency: true, hardwareAcceleration: 'prefer-hardware' };
    const ok = await VideoDecoder.isConfigSupported(config).catch(() => ({ supported: false }));
    if (!ok.supported) throw new Error(`codec ${init.codec} unsupported`);
    dec?.state !== 'closed' && dec?.close();
    dec = new VideoDecoder({
      output: (f) => { latest?.close(); latest = f; },   // only the newest frame is painted
      error: (e) => { console.warn('[live] decoder', e); ws?.close(); },
    });
    dec.configure(config);
  }

  function connect() {
    if (!('VideoDecoder' in window)) return mjpeg('browser has no WebCodecs');
    let configured = false;
    ws = new WebSocket(authURL('/video').replace(/^http/, 'ws'));
    ws.binaryType = 'arraybuffer';
    ws.onmessage = async (ev) => {
      if (typeof ev.data === 'string') {
        const m = JSON.parse(ev.data);
        if (m.t !== 'init') return;
        try { await configure(m); configured = true; } catch (e) { ws.onclose = null; ws.close(); return mjpeg(String(e)); }
        failures = 0;
        mode = 'h264'; img.hidden = true; canvas.hidden = false;
        img.removeAttribute('src');                         // stop the MJPEG download
        onMode(mode);
        requestAnimationFrame(paint);
        return;
      }
      if (!configured || dec.state !== 'configured') return;
      const data = new Uint8Array(ev.data);
      const key = isKey(data);
      // Decoder behind: drop frames, then wait for a keyframe (a delta without the frames
      // it builds on decodes to a smeared picture).
      if (dec.decodeQueueSize > 6) { needKey = true; return; }
      if (needKey && !key) return;
      needKey = false;
      dec.decode(new EncodedVideoChunk({ type: key ? 'key' : 'delta', timestamp: (ts += 16667), data }));
    };
    ws.onclose = () => {
      ws = null;
      mjpeg('socket closed');
      // Keep trying (slower each time, at most every 15 s): the engine may be restarting.
      clearTimeout(retry);
      retry = setTimeout(connect, Math.min(15000, 1000 * ++failures));
    };
  }

  connect();
  return {
    get mode() { return mode; },
    reconnect() { failures = 0; clearTimeout(retry); ws ? ws.close() : connect(); },
  };
}

function isKey(au) {
  for (let i = 0; i + 3 < au.length; i++) {
    if (au[i] === 0 && au[i + 1] === 0 && au[i + 2] === 1) {
      const t = au[i + 3] & 0x1f;
      if (t === 5 || t === 7) return true;
      if (t === 1) return false;
    }
  }
  return false;
}
