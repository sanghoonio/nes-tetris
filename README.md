# NES Tetris, rebuilt in Rust

The 1989 NES version of Tetris, rewritten from scratch as a native desktop game. It plays exactly
like the original, down to the frame, with a few comforts added on top: proper keyboard
controls, remappable keys, gamepad support, a settings screen and saved high scores.

**You need your own copy of the game.** This project contains no Nintendo or Tetris code, graphics
or music. When you build it, it reads them from a ROM file of NES Tetris (USA) that you provide.

Not affiliated with or endorsed by Nintendo or The Tetris Company.

## Playing it

### What you need

- A Mac, Linux or Windows computer. The game is built and tested on macOS; it also compiles
  for Linux and Windows, but hasn't been played on them yet.
- [Rust](https://rustup.rs), installed with the one-line command on that page (on Windows,
  the installer there, which also asks for Visual Studio's C++ build tools).
- On Linux, the development packages for sound and gamepads. On Debian or Ubuntu:

      sudo apt install pkg-config libasound2-dev libudev-dev

  (Fedora: `alsa-lib-devel systemd-devel`.)
- A ROM of **Tetris (USA)** for the NES, as a `.nes` file. The build checks that it's the right
  one and tells you if it isn't.

### Build and run

1. Download this project:

       git clone https://github.com/sanghoonio/nes-tetris.git
       cd nes-tetris

2. Copy your ROM into the folder as `tetris.nes`.

3. Run the game:

       cargo run --release

The first build takes a minute or two. The build copies the graphics, music and data tables out
of your ROM and into the game (the original's program code isn't needed and is left out), so
after that the program runs on its own without the ROM file.

If your ROM is somewhere else, point the build at it instead of copying it:

    TETRIS_ROM=/path/to/your/tetris.nes cargo run --release

(In Windows PowerShell: `$env:TETRIS_ROM="C:\path\to\tetris.nes"; cargo run --release`.)

The finished program is `target/release/tetris` (`tetris.exe` on Windows), which you can copy
anywhere and run directly.

### Make it a Mac app

To get a `Tetris.app` you can open from Finder, Launchpad or Spotlight:

    desktop/macos/bundle.sh --install

This puts the app in `~/Applications`. Without `--install`, the app is left at
`target/Tetris.app`. Drawing the app icon needs [uv](https://docs.astral.sh/uv/).

The app contains the graphics and music from your ROM, so don't share it.

## Controls

The title screen has START, SETTINGS and EXIT.

**Menus:** arrow keys to move, Enter to choose, Escape to go back. Enter also skips the opening
legal screen. On the level select, hold Shift to see levels 10 to 19, and press Shift+Enter to
start there.

**In a game** (all of these can be changed in SETTINGS > CONTROLS):

| Action | Key |
|---|---|
| Move left / right | Left / Right arrows |
| Soft drop | Down arrow |
| Hard drop (if turned on) | Space |
| Rotate clockwise | X |
| Rotate counterclockwise | Z |
| Pause | Escape |

Pausing brings up a RESUME / QUIT menu. Switching to another window pauses the game
automatically.

**High score names:** just type. Backspace deletes, Left/Right move along the name, Up/Down
cycle a letter, and Enter finishes.

**Gamepads:** the d-pad or left stick moves. In menus, the bottom face button (or Start)
chooses and the right one goes back. In a game, the right and bottom buttons rotate, the top
button hard-drops, and Start pauses.

Quit with EXIT, by closing the window, or with Cmd+Q on a Mac.

## Settings

SETTINGS has three pages:

- **CONTROLS:** change the keys used in a game.
- **GAME:** how quickly a held direction starts repeating and how fast it repeats, what happens
  when left and right are both held, the slowdown from holding down with left or right, hard
  drop, a ghost piece showing where the piece will land, showing the next piece, and whether
  to save high scores.
- **SYSTEM:** volume, music, fullscreen, pixel scale and resetting everything to the defaults.
  MUSIC is the same choice as the music screen before a game (1, 2, 3 or OFF) and plays it while
  you choose; it's also what you hear in a versus match. Pixel
  scale chooses how the picture fills the window. FIT makes it as big as possible. WHOLE only
  enlarges it by a whole number (2×, 3×…) so every pixel is the same size and perfectly sharp,
  with a wider border.

Every setting starts out the way the original game works, so out of the box it plays like the
NES. Your settings, your last game's choices and your high scores are saved in
`settings.toml`, in:

- macOS: `~/Library/Application Support/Tetris/`
- Linux: `~/.config/Tetris/`
- Windows: `%APPDATA%\Tetris\`

## Versus

An optional build adds VERSUS to the title menu: one-on-one races against another copy of the
game, as at the Classic Tetris World Championship. Each player plays their own game; when one
tops out, the other plays on until they pass that score or top out too. Passing it wins, and the
winner (a bot too) can keep playing; the result is shown when they top out or quit, or against
your own bot when you press Escape twice. Others can watch.

    cargo run --release --features versus

(For the Mac app: `desktop/macos/bundle.sh --versus --install`.)

**Playing:** HOST makes a room. JOIN lists the rooms with a seat free and WATCH lists every
room, matches being played included; choose one, or ENTER A CODE for a room that isn't listed
(the room code is at the top of its lobby; PUBLIC OFF under ROOM SETTINGS keeps a room off the
lists). The host sets the level, SAME SEED (both players get
the same pieces; on unless turned off) and the rules (DAS, hard drop, ghost and so on) under ROOM
SETTINGS in the room, where the others can look but not change them; both players
choose READY and the match starts after a countdown. The window widens to show both games side by
side, yours on the left, each with its playfield, score, next piece and level (the piece
statistics aren't shown). Each score box also shows the gap to the other player, green when
ahead and red when behind, and above each half a box shows the player's name and the matches
they've won between you.
Your game keys work as usual; there's no
pausing, and pressing Escape twice leaves the match, which forfeits it (against your own bot
you forfeit and stay in the room). After the result, READY again for a rematch.
Between matches the host can remove the other player: choose their name in the lobby, then KICK?
YES.
With the other seat free, the host can ADD BOT (beside LEAVE): a computer player from a roster
of five, weakest to strongest SLEVE, BOBSON, JEROMY, KEVIN and ANATOLI, playing under the
room's settings. Its game runs on the host's computer, so it leaves when the host does; KICK?
removes it too.

Each player needs their own ROM. Only button presses and scores travel between the games.

**Rooms** are run by a small server. To play on your own network, start one on one of the
computers:

    cargo run --release -p netplay --bin local-room

The game looks for it at `ws://localhost:8788`; to use another computer's, set `server` under
`[versus]` in `settings.toml` (for example `ws://192.168.1.20:8788`). Two copies can run on one
computer with different names:

    cargo run --release --features versus -- --name ANA
    cargo run --release --features versus -- --name BO

`--server URL` works the same way, for a single run without saving.

To host rooms on the internet, deploy the server in `server/` to Cloudflare (the free plan is
enough); it serves the web version too (below):

    cd server
    npx wrangler login
    npx wrangler deploy

It prints the address, `https://nes-tetris.<your-subdomain>.workers.dev`: the web version is
there, and the desktop app reaches the rooms by setting `server` under `[versus]` to the same
with `wss://` in front.

## In a browser

The same game, menus and VERSUS run in a web page (`web/`): the app as WebAssembly
(`web/engine`) in a small page. Each player gives it their own ROM file once; it stays in their
browser and is never uploaded. Web and desktop players meet in the same rooms. Building it needs
Node and `wasm-pack` (`cargo install wasm-pack`).

To try it on your computer, either the page alone, with rooms from `local-room`:

    cargo run --release -p netplay --bin local-room
    cd web && npm install && npm run dev        # then http://localhost:5173

or the page and the rooms together as Cloudflare would run them:

    cd server && npx wrangler dev               # then http://localhost:8787

`npm test` in `web/` checks the WebAssembly build under Node, including a match between two copies.
