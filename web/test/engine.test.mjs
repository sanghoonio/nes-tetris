// The engine as the page runs it (WebAssembly), under Node: the ROM check, menus by key name,
// settings coming back as JSON, and a match between two copies through a local room server over
// Node's own WebSocket (as the browser's).
//
//     npm test

import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { spawn } from "node:child_process";
import { initSync, Engine } from "../src/engine/engine.js";

initSync({ module: readFileSync(new URL("../src/engine/engine_bg.wasm", import.meta.url)) });
const ROM = readFileSync(new URL("../../tetris.nes", import.meta.url));
const FRAME_MS = 1000 / 60.0988;
const pause = (ms = 0) => new Promise((r) => setTimeout(r, ms));

/// A copy of the game: its own clock, keys by name.
class Copy {
  constructor(server, settings) {
    this.engine = new Engine(ROM, settings, 48000, server, Math.random() * 2 ** 52);
    this.now = 0;
  }
  frame(held = [], pressed = []) {
    this.now += FRAME_MS;
    this.engine.frame(held, pressed, false, this.now);
    this.engine.samples();
  }
  idle(n) {
    for (let i = 0; i < n; i++) this.frame();
  }
  tap(key, after = 3) {
    this.frame([key], [key]);
    this.idle(after);
  }
  width() {
    return this.engine.width();
  }
}

test("not Tetris is refused", () => {
  assert.throws(() => new Engine(new Uint8Array(1000), undefined, 48000, "", 0));
});

test("menus, and settings to keep", () => {
  const c = new Copy("");
  c.idle(10);
  c.tap("Enter", 20); // past the legal screen
  assert.equal(c.width(), 256);
  assert.equal(c.engine.rgba().length, 256 * 240 * 4);
  // SETTINGS (third on the title menu with VERSUS): to SYSTEM, volume down.
  c.tap("ArrowDown");
  c.tap("ArrowDown");
  c.tap("Enter", 20);
  for (let i = 0; i < 7; i++) c.tap("ArrowDown");
  c.tap("Enter", 5); // GAME SETTINGS
  for (let i = 0; i < 8; i++) c.tap("ArrowDown");
  c.tap("Enter", 5); // SYSTEM SETTINGS
  c.engine.changed_settings();
  c.tap("ArrowLeft");
  const saved = JSON.parse(c.engine.changed_settings());
  assert.equal(saved.volume, 0.9);
  assert.ok(Math.abs(c.engine.volume() - 0.9) < 1e-6);
  assert.equal(c.engine.changed_settings(), undefined, "only once");
  // A new copy with those settings starts with them.
  assert.ok(Math.abs(new Copy("", JSON.stringify(saved)).engine.volume() - 0.9) < 1e-6);
});

/// The local room server on a free port, until the test ends.
async function localRoom(t) {
  const port = 20000 + Math.floor(Math.random() * 20000);
  const server = spawn("cargo", ["run", "--release", "-q", "-p", "netplay", "--bin", "local-room", String(port)], {
    cwd: new URL("../..", import.meta.url),
    stdio: ["ignore", "pipe", "inherit"],
  });
  t.after(() => server.kill());
  await new Promise((resolve, reject) => {
    server.stdout.on("data", (d) => String(d).includes("rooms on") && resolve());
    server.on("exit", (code) => reject(new Error(`local-room exited: ${code}`)));
  });
  return `ws://127.0.0.1:${port}`;
}

/// Run every copy, `per` frames at a time with the event loop turning between, until `done`.
async function run(what, copies, done, { held = () => [], per = 4, limit = 60_000 } = {}) {
  const start = Date.now();
  while (!done()) {
    assert.ok(Date.now() - start < limit, `timed out waiting for ${what}`);
    for (const c of copies) for (let i = 0; i < per; i++) c.frame(held(c));
    await pause(1);
  }
}

/// Run every copy for `ms` of real time (for the network to answer).
async function wait(copies, ms) {
  const start = Date.now();
  while (Date.now() - start < ms) {
    for (const c of copies) c.frame();
    await pause(1);
  }
}

test("a match between two copies, found through JOIN's list", async (t) => {
  const server = await localRoom(t);
  const ana = new Copy(server);
  const bo = new Copy(server);
  for (const c of [ana, bo]) {
    c.idle(10);
    c.tap("Enter", 20);
    c.tap("ArrowDown"); // VERSUS
    c.tap("Enter", 20);
  }
  ana.tap("Enter", 20); // HOST
  await wait([ana], 1000);
  bo.tap("ArrowDown"); // JOIN
  bo.tap("Enter", 20);
  // Once the room is listed, the cursor is on it; Enter joins.
  await wait([ana, bo], 1500);
  bo.tap("Enter", 20);
  await wait([ana, bo], 1500);
  // Both ready (their cursors start on READY).
  ana.tap("Enter", 5);
  bo.tap("Enter", 5);
  await run("the match", [ana, bo], () => ana.width() > 256 && bo.width() > 256);
  // Drop everything straight down until both top out and the result goes.
  await run("the result and the lobby again", [ana, bo], () => ana.width() === 256 && bo.width() === 256, {
    held: () => ["ArrowDown"],
    per: 30,
  });
});
