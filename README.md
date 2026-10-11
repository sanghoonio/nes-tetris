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

       git clone https://github.com/sanghoonio/tetris_nes.git
       cd tetris_nes

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

    tetris-rs/macos/bundle.sh --install

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
- **SYSTEM:** volume, fullscreen, pixel scale and resetting everything to the defaults. Pixel
  scale chooses how the picture fills the window. FIT makes it as big as possible. WHOLE only
  enlarges it by a whole number (2×, 3×…) so every pixel is the same size and perfectly sharp,
  with a wider border.

Every setting starts out the way the original game works, so out of the box it plays like the
NES. Your settings, your last game's choices and your high scores are saved in
`settings.toml`, in:

- macOS: `~/Library/Application Support/Tetris/`
- Linux: `~/.config/Tetris/`
- Windows: `%APPDATA%\Tetris\`
