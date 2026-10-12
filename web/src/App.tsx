import { useEffect, useRef, useState } from "react";
import { checkRom, Runner } from "./game";
import { forgetRom, loadRom, saveRom } from "./rom";

export default function App() {
  // undefined while looking for a kept ROM.
  const [rom, setRom] = useState<Uint8Array | null | undefined>(undefined);
  useEffect(() => {
    loadRom().then(setRom, () => setRom(null));
  }, []);
  if (rom === undefined) return null;
  if (rom === null)
    return (
      <RomPage
        onRom={async (r) => {
          await saveRom(r);
          setRom(r);
        }}
      />
    );
  return (
    <Game
      rom={rom}
      onForget={async () => {
        await forgetRom();
        setRom(null);
      }}
    />
  );
}

/// INSERT ROM: the panel is where the ROM file goes (dropped, or chosen with a click). Asked
/// once; the browser keeps it.
function RomPage({ onRom }: { onRom: (rom: Uint8Array) => void }) {
  const [error, setError] = useState<string | null>(null);
  const [over, setOver] = useState(false);
  const input = useRef<HTMLInputElement>(null);
  useEffect(() => {
    document.title = "Insert ROM";
  }, []);
  const take = async (file: File | undefined) => {
    if (!file) return;
    const rom = new Uint8Array(await file.arrayBuffer());
    if (await checkRom(rom)) setError("This ROM won't run. It needs Tetris (USA).");
    else onRom(rom);
  };
  return (
    <div className="rom">
      <div
        className={over ? "panel over" : "panel"}
        onClick={() => input.current?.click()}
        onDragOver={(e) => {
          e.preventDefault();
          setOver(true);
        }}
        onDragLeave={() => setOver(false)}
        onDrop={(e) => {
          e.preventDefault();
          setOver(false);
          void take(e.dataTransfer.files[0]);
        }}
      >
        <p>Insert ROM</p>
        {error && <p className="error">{error}</p>}
        <input ref={input} type="file" accept=".nes" hidden onChange={(e) => void take(e.target.files?.[0])} />
      </div>
    </div>
  );
}

/// The game, from power-on. Sound starts with the first key or click (browsers allow it only
/// then). EXIT powers it on again.
function Game({ rom, onForget }: { rom: Uint8Array; onForget: () => void }) {
  const canvas = useRef<HTMLCanvasElement>(null);
  const [run, setRun] = useState(0);
  useEffect(() => {
    document.title = "Tetris";
  }, []);
  useEffect(() => {
    let runner: Runner | null = null;
    let stopped = false;
    void Runner.start(canvas.current!, rom, () => setRun((n) => n + 1)).then((r) => {
      if (stopped) r.stop();
      else runner = r;
    });
    return () => {
      stopped = true;
      runner?.stop();
    };
  }, [rom, run]);
  return (
    <>
      <div className="game">
        <canvas ref={canvas} />
      </div>
      <div className="corner">
        <button onClick={() => void document.documentElement.requestFullscreen().catch(() => {})}>Fullscreen</button>
        <button onClick={onForget}>Eject</button>
      </div>
    </>
  );
}
