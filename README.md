# NES Tetris, rebuilt in Rust

The 1989 NES version of Tetris, rewritten from scratch as a native desktop game. It plays exactly
like the original, down to the frame, with a few comforts added on top: proper keyboard
controls, remappable keys, gamepad support, a settings screen and saved high scores.

**You need your own copy of the game.** This project contains no Nintendo or Tetris code, graphics
or music. The game reads them from a ROM file of NES Tetris (USA) that you provide.

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
- A ROM of **Tetris (USA)** for the NES, as a `.nes` file. The game checks that it's the right
  one and tells you if it isn't.

### Build and run

1. Download this project:

       git clone https://github.com/sanghoonio/nes-tetris.git
       cd nes-tetris

2. Run the game:

       cargo run --release

3. The first time, it asks for your ROM: drag the file onto the window, or click to choose it.
   It keeps a copy next to its settings, so after that it starts straight away.

The first build takes a minute or two. The finished program is `target/release/tetris`
(`tetris.exe` on Windows), which you can copy anywhere and run directly. It contains nothing from
the game, so you can share it.

To build a copy that never asks, put the ROM in the folder as `tetris.nes` and add
`--features embedded-rom`. The build copies the game's graphics, music and data tables out of
your ROM into the program; the ROM file itself and the original's program code aren't included.
Don't distribute this copy.

### Make it a Mac app

To get a `Tetris.app` you can open from Finder, Launchpad or Spotlight:

    desktop/macos/bundle.sh --install

This puts the app in `~/Applications`. Without `--install`, the app is left at
`target/Tetris.app`. Drawing the app icon needs [uv](https://docs.astral.sh/uv/). Like the
program, it asks for your ROM the first time. Add `--embed-rom` to build in your ROM's graphics,
music and data instead (as `embedded-rom` above), and don't share that one.

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

The ROM you gave it is kept in the same folder as `tetris.nes`. Delete it to be asked again.

## Versus

Versus lets you race another player head to head, as played at CTWC. You each play your own game
side by side. When one of you tops out, the other has to pass their score to win, or tops out
too. The winner can keep playing to push their score up. Other people can join a room to watch.

It's an optional part of the build:

    cargo run --release --features versus

For the Mac app, use `desktop/macos/bundle.sh --versus --install`.

Both players need their own ROM.

### Finding a match

Pick VERSUS on the title screen. HOST makes a new room. JOIN shows rooms that have a free seat,
and WATCH shows all of them, including matches in progress. If a room isn't listed, use ENTER A
CODE with the code from the top of its lobby. Left and Right move a page at a time through a
long list, and Right from the last page goes to ENTER A CODE. OFFLINE plays against the computer
without going online.

### In the room

The host sets up the match in ROOM SETTINGS: starting level, whether both players get the same
pieces, and rules like hard drop and the ghost piece. Turning PUBLIC off hides the room from the
lists, so only people with the code can get in.

Once both players hit READY there's a countdown and the match starts. Hit READY again afterwards
for a rematch. Between matches the host can kick the other player by selecting their name.

Rooms fit two players and 16 spectators.

### During a match

The window widens to fit both games, with yours on the left. Above each game is the player's
name and how many matches they've won. Next to each score is how far ahead (green) or behind
(red) that player is.

You can't pause against another person. Pressing Escape twice leaves, and counts as a loss.
Against the computer, Escape pauses as normal.

Matches run on the clock. If your game stops (you switch tabs, your computer stalls), it catches
up when it's back, with the pieces in the meantime falling unsteered. If your moves stop getting
through, your game carries on, and the other player sees NO SIGNAL over it. If
your moves are 10 seconds behind, you lose the match and drop back to the lobby.

### Playing the computer

If the other seat is empty, the host can pick ADD CPU. There are five computer players, from
weakest to strongest: SLEVE, BOBSON, JEROMY, KEVIN and ANATOLI. They play on the host's
computer, so they leave when the host does.

### Playing on your own network

By default the game finds rooms through a server on the internet. You can run your own on one
computer instead:

    cargo run --release -p netplay --bin local-room

Then set `server` under `[versus]` in `settings.toml` to `ws://localhost:8788` on that computer,
or to its address on the others, like `ws://192.168.1.20:8788`.

To try it out on one computer, open two copies with different names:

    cargo run --release --features versus -- --name ANA
    cargo run --release --features versus -- --name BO

## In a browser

You can also play at **https://nes-tetris.online**. Load your ROM the first time.
Browser and desktop players can play each other.

## For developers

### The web page

The page is in `web/`. You'll need [Node](https://nodejs.org) and `wasm-pack`
(`cargo install wasm-pack`). To run it locally, start a room server and the page:

    cargo run --release -p netplay --bin local-room
    cd web && npm install && npm run dev        # then open http://localhost:5173

Run the page's tests with `npm test` in `web/`.

### The server

The room server in `server/` runs on Cloudflare's free plan and serves the web page too. Pushing
to `main` tests it and deploys it (`.github/workflows/ci.yml`). To deploy by hand:

    cd server
    npx wrangler login
    npx wrangler deploy

To run the server and page locally the way Cloudflare would:

    cd server && npx wrangler dev               # then open http://localhost:8787
