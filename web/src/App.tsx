import { useEffect, useRef, useState } from "react";
import { checkRom, Runner } from "./game";
import { forgetRom, loadRom, saveRom } from "./rom";

const SOURCE = "https://github.com/sanghoonio/nes-tetris";

export default function App() {
  // undefined while looking for a kept ROM.
  const [rom, setRom] = useState<Uint8Array | null | undefined>(undefined);
  // What's open over the page: the game's menu, the info panel, or the keys.
  const [panel, setPanel] = useState<"menu" | "info" | "help" | null>(null);
  const runner = useRef<Runner | null>(null);
  useEffect(() => {
    loadRom().then(setRom, () => setRom(null));
  }, []);
  if (rom === undefined) return null;
  const close = () => setPanel(null);
  const eject = async () => {
    await forgetRom();
    setRom(null);
  };
  return (
    <>
      {rom === null ? (
        <RomPage
          onRom={async (r) => {
            await saveRom(r);
            setRom(r);
          }}
          links={
            <>
              <button onClick={() => setPanel("info")}>Info</button>
              <a href={SOURCE} target="_blank" rel="noreferrer">
                GitHub
              </a>
            </>
          }
        />
      ) : (
        <>
          <Game rom={rom} free={panel === null} runner={runner} />
          {panel === null && <Corner onHelp={() => setPanel("help")} onMenu={() => setPanel("menu")} />}
        </>
      )}
      {panel === "menu" && (
        <Panel onClose={close}>
          <div className="menu">
            <button onClick={() => setPanel("info")}>Info</button>
            <a href={SOURCE} target="_blank" rel="noreferrer">
              GitHub
            </a>
            <button
              onClick={() => {
                close();
                void document.documentElement.requestFullscreen().catch(() => {});
              }}
            >
              Fullscreen
            </button>
            <button
              onClick={() => {
                close();
                void eject();
              }}
            >
              Eject
            </button>
            <button className="back" onClick={close}>
              Back
            </button>
          </div>
        </Panel>
      )}
      {panel === "info" && <Info onClose={close} />}
      {panel === "help" && <Help controls={runner.current?.controls() ?? []} onClose={close} />}
    </>
  );
}

/// INSERT ROM: the panel is where the ROM file goes (dropped, or chosen with a click). Asked
/// once; the browser keeps it.
function RomPage({ onRom, links }: { onRom: (rom: Uint8Array) => void; links: React.ReactNode }) {
  const [error, setError] = useState<string | null>(null);
  const [over, setOver] = useState(false);
  const input = useRef<HTMLInputElement>(null);
  useEffect(() => {
    document.title = "Insert ROM";
  }, []);
  const take = async (file: File | undefined) => {
    if (!file) return;
    const rom = new Uint8Array(await file.arrayBuffer());
    if (await checkRom(rom)) setError("This ROM won't run.");
    else onRom(rom);
  };
  return (
    <div className="screen">
      <div
        className={over ? "panel drop over" : "panel drop"}
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
        <p className="dim">NES Tetris (USA)</p>
        {error && <p className="error">{error}</p>}
        <input ref={input} type="file" accept=".nes" hidden onChange={(e) => void take(e.target.files?.[0])} />
      </div>
      <div className="links">{links}</div>
    </div>
  );
}

/// HELP and MENU in the corner over the game: as the other links while the mouse moves, fainter
/// after it's been still a couple of seconds (the game is played with keys).
function Corner({ onHelp, onMenu }: { onHelp: () => void; onMenu: () => void }) {
  const [shown, setShown] = useState(true);
  useEffect(() => {
    let timer = window.setTimeout(() => setShown(false), 2000);
    const moved = () => {
      setShown(true);
      window.clearTimeout(timer);
      timer = window.setTimeout(() => setShown(false), 2000);
    };
    window.addEventListener("pointermove", moved);
    return () => {
      window.clearTimeout(timer);
      window.removeEventListener("pointermove", moved);
    };
  }, []);
  return (
    <div className={shown ? "links corner" : "links corner hidden"}>
      <button onClick={onHelp}>Help</button>
      <button onClick={onMenu}>Menu</button>
    </div>
  );
}

/// A framed panel over the page. Escape or a click outside closes it.
function Panel({ onClose, children }: { onClose: () => void; children: React.ReactNode }) {
  useEffect(() => {
    const key = (e: KeyboardEvent) => e.code === "Escape" && onClose();
    window.addEventListener("keydown", key);
    return () => window.removeEventListener("keydown", key);
  }, [onClose]);
  return (
    <div className="screen modal" onClick={onClose}>
      <div className="panel" onClick={(e) => e.stopPropagation()}>
        {children}
      </div>
    </div>
  );
}

/// What this is, and where the source is.
function Info({ onClose }: { onClose: () => void }) {
  return (
    <Panel onClose={onClose}>
      <div className="info">
        <p>NES Tetris, rebuilt in Rust, using assets from your own ROM. Play against others or CPU in versus mode.</p>
        <p>
          Source:{" "}
          <a href={SOURCE} target="_blank" rel="noreferrer">
            github.com/sanghoonio/nes-tetris
          </a>
        </p>
        <button onClick={onClose}>Back</button>
      </div>
    </Panel>
  );
}

/// The keys: the game's as set (SETTINGS > CONTROLS changes them), in two columns, moving on
/// the left and rotating and pausing on the right; hard drop is off unless turned on, so left out.
function Help({ controls, onClose }: { controls: [string, string][]; onClose: () => void }) {
  const shown = controls.filter(([action]) => action !== "HARD DROP");
  const half = Math.ceil(shown.length / 2);
  const rows = shown.slice(0, half).map((left, i) => [left, shown[half + i]]);
  return (
    <Panel onClose={onClose}>
      <div className="help">
        <table>
          <tbody>
            {rows.map(([left, right]) => (
              <tr key={left[0]}>
                <td>{left[0]}</td>
                <td>{left[1]}</td>
                <td>{right?.[0]}</td>
                <td>{right?.[1]}</td>
              </tr>
            ))}
          </tbody>
        </table>
        <p>Use arrow keys, Enter, and Esc to navigate.</p>
        <button onClick={onClose}>Back</button>
      </div>
    </Panel>
  );
}

/// The game, from power-on. Sound starts with the first key or click (browsers allow it only
/// then). EXIT powers it on again. While a panel is up (`free` false) it takes no keys, and
/// stops where it is unless in a VERSUS room.
function Game({ rom, free, runner }: { rom: Uint8Array; free: boolean; runner: React.RefObject<Runner | null> }) {
  const canvas = useRef<HTMLCanvasElement>(null);
  const [run, setRun] = useState(0);
  useEffect(() => {
    document.title = "Tetris";
  }, []);
  useEffect(() => {
    let stopped = false;
    void Runner.start(canvas.current!, rom, () => setRun((n) => n + 1)).then((r) => {
      if (stopped) r.stop();
      else runner.current = r;
    });
    return () => {
      stopped = true;
      runner.current?.stop();
      runner.current = null;
    };
  }, [rom, run]);
  useEffect(() => {
    runner.current?.setKeys(free);
    runner.current?.setPaused(!free);
  }, [free]);
  return (
    <div className="game">
      <canvas ref={canvas} />
    </div>
  );
}
