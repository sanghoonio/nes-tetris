// The game on the page: the engine (the app as WebAssembly) stepped at the NES's 60.0988 frames
// a second, drawn to a canvas, its sound through an AudioWorklet, keys and gamepads by the names
// the engine knows (`KeyboardEvent.code`, `PadSouth`...), and its settings kept in localStorage.

import init, { Engine } from "./engine/engine.js";

const FRAME_MS = 1000 / 60.0988;
/// Frames after pausing in which the pause screen is drawn and its sound plays out (18).
const PAUSE_SETTLE_FRAMES = 20;
const SETTINGS = "nes-tetris.settings";

let ready: Promise<unknown> | null = null;
function loadEngine() {
  ready ??= init();
  return ready;
}

/// The room server: `?server=` if given; the local room server beside Vite's dev server; else the
/// server that served this page.
export function serverAddress(): string {
  const given = new URLSearchParams(location.search).get("server");
  if (given) return given;
  if (location.port === "5173") return `ws://${location.hostname}:8788`;
  return `${location.protocol === "https:" ? "wss" : "ws"}://${location.host}`;
}

/// Whether `rom` is Tetris (USA); the reason if not.
export async function checkRom(rom: Uint8Array): Promise<string | null> {
  await loadEngine();
  try {
    new Engine(rom, undefined, 48000, "", 0).free();
    return null;
  } catch (e) {
    return String(e instanceof Error ? e.message : e);
  }
}

/// Gamepad buttons (the standard mapping) and the left stick, as the desktop names them.
function padsHeld(): Set<string> {
  const held = new Set<string>();
  for (const pad of navigator.getGamepads?.() ?? []) {
    if (!pad || pad.mapping !== "standard") continue;
    const b = (i: number) => pad.buttons[i]?.pressed;
    const [x, y] = [pad.axes[0] ?? 0, pad.axes[1] ?? 0];
    const names: [boolean, string][] = [
      [b(12) || y < -0.5, "PadUp"],
      [b(13) || y > 0.5, "PadDown"],
      [b(14) || x < -0.5, "PadLeft"],
      [b(15) || x > 0.5, "PadRight"],
      [b(0), "PadSouth"],
      [b(1), "PadEast"],
      [b(2), "PadWest"],
      [b(3), "PadNorth"],
      [b(9), "PadStart"],
      [b(8), "PadSelect"],
    ];
    for (const [on, name] of names) if (on) held.add(name);
  }
  return held;
}

export class Runner {
  private engine: Engine;
  private draw: CanvasRenderingContext2D;
  private audio: AudioContext;
  private sound: AudioWorkletNode;
  private gain: GainNode;
  private held = new Set<string>();
  private pressed: string[] = [];
  private pads = new Set<string>();
  private focused = true;
  /// Whether the game takes the keyboard, and whether it's paused (for something else on the page).
  private keys = true;
  private paused = false;
  private frame = 0;
  private last = 0;
  private behind = 0;
  private stopped = false;
  private width = 0;
  private height = 0;
  private listeners: [EventTarget, string, EventListener][] = [];

  private constructor(
    private canvas: HTMLCanvasElement,
    engine: Engine,
    audio: AudioContext,
    sound: AudioWorkletNode,
    gain: GainNode,
    private onQuit: () => void,
  ) {
    this.engine = engine;
    this.audio = audio;
    this.sound = sound;
    this.gain = gain;
    this.draw = canvas.getContext("2d")!;
  }

  /// Start the game on `canvas`. Its sound starts with the first key, click or touch: browsers
  /// allow sound only once the page has been used.
  static async start(canvas: HTMLCanvasElement, rom: Uint8Array, onQuit: () => void): Promise<Runner> {
    const audio = new AudioContext();
    await audio.audioWorklet.addModule("/audio.js");
    const sound = new AudioWorkletNode(audio, "samples", { outputChannelCount: [2] });
    const gain = audio.createGain();
    sound.connect(gain).connect(audio.destination);
    await loadEngine();
    const settings = localStorage.getItem(SETTINGS) ?? undefined;
    const engine = new Engine(rom, settings, audio.sampleRate, serverAddress(), Math.random() * 2 ** 52);
    const runner = new Runner(canvas, engine, audio, sound, gain, onQuit);
    runner.gain.gain.value = engine.volume();
    runner.listen();
    if (audio.state !== "running") {
      const resume = () => void audio.resume();
      for (const type of ["keydown", "pointerdown", "touchstart"]) runner.on(window, type, resume);
    }
    runner.frame = requestAnimationFrame((t) => runner.tick(t));
    return runner;
  }

  /// Give the game the keyboard, or take it away (for something else on the page).
  setKeys(on: boolean) {
    this.keys = on;
    if (!on) this.held.clear();
  }

  /// The game's keys as set: action and key, as the settings screen names them.
  controls(): [string, string][] {
    return JSON.parse(this.engine.controls());
  }

  /// Stop the game where it is, or carry on. Not in a VERSUS room: the others play on.
  setPaused(on: boolean) {
    this.paused = on;
  }

  stop() {
    if (this.stopped) return;
    this.stopped = true;
    cancelAnimationFrame(this.frame);
    for (const [target, type, f] of this.listeners) target.removeEventListener(type, f);
    void this.audio.close();
    this.engine.free();
  }

  private on(target: EventTarget, type: string, f: EventListener) {
    target.addEventListener(type, f);
    this.listeners.push([target, type, f]);
  }

  private listen() {
    this.on(window, "keydown", (e) => {
      const k = e as KeyboardEvent;
      // Leave the browser's own keys alone (F11, reload, devtools...).
      if (!this.keys || /^F\d+$/.test(k.code) || k.metaKey || k.ctrlKey) return;
      k.preventDefault();
      if (!k.repeat) {
        this.held.add(k.code);
        this.pressed.push(k.code);
      }
    });
    this.on(window, "keyup", (e) => this.held.delete((e as KeyboardEvent).code));
    // Releases while away never arrive.
    this.on(window, "blur", () => {
      this.held.clear();
      this.focused = false;
    });
    this.on(window, "focus", () => (this.focused = true));
    // A hidden tab gets no more animation frames, so the frames in which losing focus pauses the
    // game (or a match against the CPU) never come: run them now, a second's worth at most, then
    // the pause's own (its screen, and its sound's quarter second). The sound goes as one piece,
    // which the worklet plays whole; frames one by one, it would keep only the last tenth of a
    // second. Then the picture, paused, is what the tab shows until it's back.
    this.on(document, "visibilitychange", () => {
      if (!document.hidden) return;
      this.held.clear();
      this.focused = false;
      if (!this.engine.pausable()) return;
      for (let i = 0; i < 60 && this.engine.pausable(); i++) this.step(false);
      for (let i = 0; i < PAUSE_SETTLE_FRAMES; i++) this.step(false);
      const samples = this.engine.samples();
      if (samples.length) this.sound.port.postMessage(samples, [samples.buffer]);
      this.paint();
    });
    this.on(window, "resize", () => this.fit());
  }

  private tick(t: number) {
    if (this.stopped) return;
    this.frame = requestAnimationFrame((t) => this.tick(t));
    if (this.paused && !this.engine.online()) {
      // Carry on from here afterwards, not with the time missed.
      this.last = 0;
      return;
    }
    this.behind += this.last ? t - this.last : FRAME_MS;
    this.last = t;
    // Catch up at most a few frames after a stall rather than fast-forwarding.
    if (this.behind > FRAME_MS * 4) this.behind = FRAME_MS;
    let stepped = false;
    while (this.behind >= FRAME_MS) {
      this.behind -= FRAME_MS;
      this.step();
      stepped = true;
      if (this.stopped) return;
    }
    if (stepped) this.paint();
  }

  /// One frame; its sound goes to the worklet unless `sound` is false (left in the engine).
  private step(sound = true) {
    const pads = padsHeld();
    for (const p of pads) if (!this.pads.has(p)) this.pressed.push(p);
    this.pads = pads;
    const held = [...this.held, ...pads];
    this.engine.frame(held, this.pressed, !this.focused, performance.now());
    this.pressed = [];
    if (sound) {
      const samples = this.engine.samples();
      if (samples.length) this.sound.port.postMessage(samples, [samples.buffer]);
    }
    const settings = this.engine.changed_settings();
    if (settings) localStorage.setItem(SETTINGS, settings);
    if (this.engine.display_changed()) {
      this.gain.gain.value = this.engine.volume();
      const want = this.engine.fullscreen();
      if (want && !document.fullscreenElement) document.documentElement.requestFullscreen().catch(() => {});
      if (!want && document.fullscreenElement) document.exitFullscreen().catch(() => {});
      this.fit();
    }
    if (this.engine.quit()) {
      this.stop();
      this.onQuit();
    }
  }

  private paint() {
    const [width, height] = [this.engine.width(), this.engine.height()];
    if (width !== this.width || height !== this.height) {
      [this.width, this.height] = [width, height];
      this.canvas.width = width;
      this.canvas.height = height;
      this.fit();
    }
    const rgba = this.engine.rgba();
    const pixels = new Uint8ClampedArray(rgba.buffer as ArrayBuffer, rgba.byteOffset, rgba.length);
    this.draw.putImageData(new ImageData(pixels, width, height), 0, 0);
  }

  /// Size the canvas to the window, keeping its shape: as big as fits, or whole multiples only.
  private fit() {
    if (!this.width) return;
    let scale = Math.min(innerWidth / this.width, innerHeight / this.height);
    if (this.engine.integer_scale()) scale = Math.max(1, Math.floor(scale));
    this.canvas.style.width = `${Math.floor(this.width * scale)}px`;
    this.canvas.style.height = `${Math.floor(this.height * scale)}px`;
  }
}
