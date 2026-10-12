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

/// Asks for the player's ROM, once.
function RomPage({ onRom }: { onRom: (rom: Uint8Array) => void }) {
  const [error, setError] = useState<string | null>(null);
  const [over, setOver] = useState(false);
  const input = useRef<HTMLInputElement>(null);
  const take = async (file: File | undefined) => {
    if (!file) return;
    const rom = new Uint8Array(await file.arrayBuffer());
    const problem = await checkRom(rom);
    if (problem) setError(`That isn't Tetris (USA): ${problem}`);
    else onRom(rom);
  };
  return (
    <div className="rom">
      <h1>Tetris</h1>
      <p>
        NES Tetris, rebuilt to run in your browser. It needs your own copy of the game: a ROM file of Tetris (USA),
        usually named <code>tetris.nes</code>.
      </p>
      <div
        className={over ? "drop over" : "drop"}
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
        Drop the ROM here, or click to choose it
        <input ref={input} type="file" accept=".nes" hidden onChange={(e) => void take(e.target.files?.[0])} />
      </div>
      {error && <p className="error">{error}</p>}
      <p>The file stays in this browser, so you won't be asked again. It's never uploaded anywhere.</p>
    </div>
  );
}

/// The game, started by a click (browsers start sound only then).
function Game({ rom, onForget }: { rom: Uint8Array; onForget: () => void }) {
  const canvas = useRef<HTMLCanvasElement>(null);
  const runner = useRef<Runner | null>(null);
  const [state, setState] = useState<"waiting" | "playing" | "ended">("waiting");
  useEffect(() => () => runner.current?.stop(), []);
  const start = async () => {
    setState("playing");
    runner.current = await Runner.start(canvas.current!, rom, () => setState("ended"));
  };
  return (
    <>
      <div className="game">
        <canvas ref={canvas} />
      </div>
      {state !== "playing" && (
        <div className="overlay">
          <button onClick={() => void start()} autoFocus>
            {state === "waiting" ? "Play" : "Play again"}
          </button>
        </div>
      )}
      <div className="corner">
        <button onClick={() => void document.documentElement.requestFullscreen().catch(() => {})}>Fullscreen</button>
        <button
          onClick={() => {
            runner.current?.stop();
            onForget();
          }}
        >
          Use another ROM
        </button>
      </div>
    </>
  );
}
