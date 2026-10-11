// Draws the waveforms of the reference pages, with the colours of tokens.css: a sketch of
// the look, not the app's painter, on synthesised music. A classic script, since modules do
// not load from file://: it defines window.PrismeSketch.

(() => {
  const token = (name) =>
    getComputedStyle(document.documentElement).getPropertyValue(name).trim();

  /** A track as band energies [low, mid, high] in 0..1 at any time (s). */
  function track(bpm, sections, seed) {
    const at = (t) =>
      (sections.find((s) => t >= s[0] && t < s[1]) ??
        sections[sections.length - 1])[2];
    return (t) => {
      const s = at(t);
      const beat = (t * bpm) / 60;
      const ph = beat % 1;
      const off = (ph + 0.5) % 1;
      const kick = s.k ? Math.exp(-ph * 11) : 0;
      const bass = s.b ? 0.55 * Math.exp(-off * 5) + 0.12 : 0;
      const clap =
        s.c && Math.floor(beat) % 2 === 1 ? 0.55 * Math.exp(-ph * 13) : 0;
      const pad = s.p
        ? s.p *
          (0.55 +
            0.25 * Math.sin(t * 0.7 + seed) +
            0.1 * Math.sin(t * 5.3 + seed))
        : 0;
      const hat = s.h
        ? 0.55 * Math.exp(-off * 18) + 0.16 * Math.exp(-((beat * 4) % 1) * 22)
        : 0;
      const noise = Math.sin(t * 913.7 + seed) * Math.sin(t * 377.1) * 0.05;
      return [
        Math.min(1, 0.88 * kick + bass + 0.03),
        Math.min(1, pad + clap + 0.18 * kick + 0.06 + noise),
        Math.min(1, hat + 0.05 + (s.a ?? 0) + noise),
      ];
    };
  }

  function surface(canvas) {
    const scale = window.devicePixelRatio > 1 ? window.devicePixelRatio : 2;
    const w = canvas.clientWidth;
    const h = canvas.clientHeight;
    canvas.width = Math.round(w * scale);
    canvas.height = Math.round(h * scale);
    const g = canvas.getContext("2d");
    g.scale(scale, scale);
    return { g, w, h };
  }

  /** The bands mixed like light: low lights the red, mid the green, high the blue. */
  function color([low, mid, high]) {
    const max = Math.max(low, mid, high, 1e-6);
    return `rgb(${Math.round((low / max) * 255)},${Math.round((mid / max) * 230)},${Math.round((high / max) * 255)})`;
  }

  /** One slice: the envelope as high as the loudest band, in the colour of the mix. */
  function slice(g, x, mid, h, bands) {
    const peak = Math.min(1, Math.max(...bands));
    g.fillStyle = color(bands);
    g.fillRect(x, mid - peak * h * 0.47, 1, peak * h * 0.94);
  }

  /** The playhead: the only white line, dispersed on its edges like light through a prism. */
  function playhead(g, x, h) {
    g.fillStyle = token("--pr-f-low");
    g.globalAlpha = 0.75;
    g.fillRect(x - 2, 0, 1, h);
    g.fillStyle = token("--pr-f-high");
    g.globalAlpha = 0.85;
    g.fillRect(x + 2, 0, 1, h);
    g.globalAlpha = 1;
    g.fillStyle = token("--pr-light");
    g.fillRect(x - 1, 0, 2, h);
  }

  /** The scrolling waveform: `pos` seconds at the playhead, `span` seconds visible. */
  function scrolling(
    canvas,
    fn,
    { bpm, pos, span, cues = [], loop = null, playheadAt = 0.5 },
  ) {
    const { g, w, h } = surface(canvas);
    g.fillStyle = token("--pr-void");
    g.fillRect(0, 0, w, h);
    const mid = h / 2;
    const x0 = w * playheadAt;
    const spp = span / w;
    if (loop) {
      const a = x0 + (loop[0] - pos) / spp;
      const b = x0 + (loop[1] - pos) / spp;
      g.fillStyle = token("--pr-light-veil");
      g.fillRect(a, 0, b - a, h);
      g.fillStyle = token("--pr-light");
      g.fillRect(a, 0, 1, h);
      g.fillRect(b - 1, 0, 1, h);
    }
    for (let x = 0; x < w; x++) {
      const t = pos + (x - x0) * spp;
      if (t < 0) continue;
      g.globalAlpha = x < x0 ? 0.42 : 1; // already played
      slice(g, x, mid, h, fn(t));
    }
    g.globalAlpha = 1;
    if (bpm) {
      const beat = 60 / bpm;
      for (
        let b = Math.ceil((pos - x0 * spp) / beat);
        b * beat < pos + (w - x0) * spp;
        b++
      ) {
        const x = Math.round(x0 + (b * beat - pos) / spp) + 0.5;
        const bar = b % 4 === 0;
        g.fillStyle = token("--pr-text");
        g.globalAlpha = bar ? 0.75 : 0.35;
        const len = bar ? 9 : 5;
        g.fillRect(x, 0, 1, len);
        g.fillRect(x, h - len, 1, len);
      }
      g.globalAlpha = 1;
    }
    for (const [time, n] of cues) {
      const x = x0 + (time - pos) / spp;
      if (x < -20 || x > w) continue;
      g.fillStyle = token("--pr-light");
      g.fillRect(x, 0, 1, h);
      g.fillRect(x, 0, 13, 13);
      g.fillStyle = token("--pr-void");
      g.font = `600 10px ${token("--pr-font-mono")}`;
      g.fillText(String(n), x + 3, 10);
    }
    playhead(g, x0, h);
  }

  /** The whole track in the same colours: the played part dimmed, the cues as ticks. */
  function overview(canvas, fn, { length, pos, cues = [] }) {
    const { g, w, h } = surface(canvas);
    g.fillStyle = token("--pr-base");
    g.fillRect(0, 0, w, h);
    const mid = h / 2;
    for (let x = 0; x < w; x++) {
      let loudest = [0, 0, 0];
      for (let k = 0; k < 6; k++) {
        const bands = fn(((x + k / 6) / w) * length);
        if (Math.max(...bands) > Math.max(...loudest)) loudest = bands;
      }
      g.globalAlpha = (x / w) * length < pos ? 0.35 : 1;
      slice(g, x, mid, h, loudest);
    }
    g.globalAlpha = 1;
    g.fillStyle = token("--pr-light");
    for (const [time] of cues) g.fillRect((time / length) * w, 0, 1, 5);
    g.fillRect((pos / length) * w - 1, 0, 2, h);
  }

  /** The beats of A (top) and B (bottom) face to face around the playhead. */
  function phase(canvas, { bpm, span, offsetMs }) {
    const { g, w, h } = surface(canvas);
    g.fillStyle = token("--pr-base");
    g.fillRect(0, 0, w, h);
    const spp = span / w;
    const x0 = w / 2;
    const beat = 60 / bpm;
    const ticks = (shift, y0, y1) => {
      for (let b = -8; b <= 8; b++) {
        const x = Math.round(x0 + (b * beat + shift) / spp) + 0.5;
        g.fillStyle = token(b % 4 === 0 ? "--pr-light" : "--pr-text-2");
        g.fillRect(x, y0, 1, y1 - y0);
      }
    };
    ticks(0, 3, h / 2 - 1);
    ticks(-offsetMs / 1000, h / 2 + 1, h - 3);
    g.fillStyle = token("--pr-line");
    g.fillRect(0, h / 2, w, 1);
    g.fillStyle = token("--pr-light");
    g.fillRect(x0 - 1, 0, 2, h);
  }

  const future = track(
    125,
    [
      [0, 31, { k: 1, h: 1 }],
      [31, 62, { k: 1, h: 1, b: 1, c: 1 }],
      [62, 150, { k: 1, h: 1, b: 1, c: 1, p: 0.35 }],
      [150, 200, { p: 0.6, a: 0.15 }],
      [200, 300, { k: 1, h: 1, b: 1, c: 1, p: 0.45, a: 0.08 }],
      [300, 382, { k: 1, h: 1, b: 1, c: 1, p: 0.25 }],
      [382, 434, { k: 1, h: 1 }],
    ],
    1.3,
  );

  const schleierwolken = track(
    125,
    [
      [0, 46, { k: 1, h: 1, p: 0.15 }],
      [46, 108, { k: 1, h: 1, b: 1, p: 0.3 }],
      [108, 170, { k: 1, h: 1, b: 1, c: 1, p: 0.4 }],
      [170, 226, { p: 0.65, a: 0.2 }],
      [226, 340, { k: 1, h: 1, b: 1, c: 1, p: 0.5 }],
      [340, 433, { k: 1, h: 1, b: 1 }],
    ],
    4.1,
  );

  window.PrismeSketch = {
    track,
    scrolling,
    overview,
    phase,
    future,
    schleierwolken,
  };
})();
