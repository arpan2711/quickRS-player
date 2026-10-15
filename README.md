<div align="center">

# quickRS

**A free practice player for your own Rocksmith 2014 songs.**
Plug in your guitar, pick a song, and play along to tab that scrolls with the music.

[**⬇ Download for Windows**](https://github.com/arpan2711/quickRS-player/releases/latest) · [Get your songs in](#get-your-songs-in) · [Converter](#the-converter-psarc--feedpak) · [Build it yourself](#build-it-yourself) · [☕ Support it](https://ko-fi.com/arpanmitra)

![quickRS playing Pride and Joy](screenshots/player-light.png)

</div>

## Get your songs in

**You don't need to convert anything.** quickRS plays your Rocksmith songs as they are, straight from Rocksmith's
own folder. It only needs Rocksmith's two keys, in a file called `keys.json` next to `quickrs.exe`. You make that
file once, in step 2. (The converter further down is optional.)

**You need:** Windows 10 or 11 (64-bit), your guitar (a Real Tone Cable, an audio interface, or a mic), and your own
Rocksmith 2014 songs. **No songs are included.**

Setting up takes four steps: download quickRS, add the keys, start quickRS, check your songs open.

Words you'll see below:

- **.psarc file:** the kind of file Rocksmith keeps its songs in. Most hold one song. Some hold many: the game's own
  `songs.psarc` has its 70 songs, and song packs hold several.
- **dlc folder:** the folder where Rocksmith keeps your .psarc files. It's called `dlc`, and it's inside Rocksmith's
  game folder, `Rocksmith2014`. With Steam it's usually
  `C:\Program Files (x86)\Steam\steamapps\common\Rocksmith2014\dlc`.
- **keys:** two codes that unlock .psarc files. Step 2 explains them.
- **F5, F2:** keys on the top row of your keyboard. On some laptops you hold *Fn* while you press them.
- **File name endings** (`.exe`, `.json`, `.txt`): Windows often hides them. So `quickrs.exe` may show as just
  `quickrs`, and `keys.example.json` as `keys.example`. That's normal.

### Step 1: Download quickRS

1. Open [Releases](https://github.com/arpan2711/quickRS-player/releases/latest).
2. Under *Assets*, click `quickRS-<version>-windows-x64.zip` (for example `quickRS-0.1.0-windows-x64.zip`). Don't
   click *Source code*.
3. Open your *Downloads* folder.
4. Right-click the zip and choose *Extract All…*, then click *Extract*.
5. In the folder that opens, double-click the folder called `quickRS-<version>` (for example `quickRS-0.1.0`).

You're now in **the quickRS folder**: the folder with `quickrs.exe` in it. There's no installer. quickRS runs from
this folder, and you can move the whole folder anywhere you like.

The quickRS folder has these files. To play, you only need `quickrs.exe` and the `keys.json` you make in step 2.

| File | What it's for |
|---|---|
| `quickrs.exe` | the player: this is quickRS |
| `keys.example.json` | the file you turn into `keys.json` in step 2 |
| `make_shortcut.ps1` | puts a quickRS shortcut on your Desktop |
| `quickRS Converter.exe` | converts your .psarc songs to feedpaks, an open format (optional, see [the converter](#the-converter-psarc--feedpak)) |
| `Convert songs.bat` | the same conversion, in a console window (optional) |
| `quickrs-cli.exe` | command-line tools (the converter, song info, tabs) |
| `README.txt` | these instructions, offline |
| `licences` | the licences |

### Step 2: Add Rocksmith's two keys

Rocksmith's .psarc files are locked (encrypted). quickRS needs two keys to open them. About the keys:

- Their names are the **PSARC key** and the **SNG key for PC**.
- Each one is a code 64 characters long, made only of the digits `0`-`9` and the letters `A`-`F`.
- They're the same in every copy of Rocksmith. They are **not** a licence key, and they have nothing to do with your
  Steam account.
- quickRS can't include them. They're published in open-source Rocksmith modding tools.

Find both keys first. Then make `keys.json` in the quickRS folder, next to `quickrs.exe`:

1. In the quickRS folder, right-click `keys.example.json` and choose *Open with* → *Notepad*.
2. Look at what's inside. It's two lines between `{` and `}`, like this (your example text is different):

   ```
   {
     "psarc_key": "<example text>",
     "sng_key": "<example text>"
   }
   ```

3. Copy the **PSARC key** from where you found it: select it and press *Ctrl+C*.
4. In Notepad, on the `"psarc_key"` line, select the example text, from `<` to `>`, both included. Don't select
   the quotes around it.
5. Press *Ctrl+V*. The key takes the place of the example text.
6. Copy the **SNG key for PC** the same way: select it and press *Ctrl+C*.
7. In Notepad, on the `"sng_key"` line, select the example text, from `<` to `>`.
8. Press *Ctrl+V*.
9. Check both lines. Each key sits between its two quotes, on one line. There's no `<` or `>` left. The quotes,
   the colons, and the comma at the end of the first line are still there. (A key written as `0x1A, 0x2B, …`
   works too.)
10. Click *File* → *Save as*.
11. Check that the Save window is in the quickRS folder: its name (for example `quickRS-0.1.0`) shows at the top.
    If it isn't, go to the quickRS folder.
12. In the *File name* box, delete what's there and type `"keys.json"` **with the quotes**. The quotes stop Windows
    from adding `.txt` to the name.
13. Click *Save*.
14. Close Notepad.

`keys.json` is now in the quickRS folder, next to `quickrs.exe`. Leave `keys.example.json` where it is.

### Step 3: Start quickRS

> [!IMPORTANT]
> **Already opened quickRS before adding the keys? Close it, open it again, then press F5.**
>
> quickRS reads `keys.json` only when it starts. Songs it saw without the keys stay marked *needs keys*, even after
> you open it again, until you press **F5**. F5 makes quickRS read your song folders again. F5 in a quickRS window
> that was open while you made `keys.json` doesn't help: close it and open it first.
>
> Never opened quickRS before making `keys.json`? Then you don't need F5.

1. If quickRS is open, close it.
2. In the quickRS folder, double-click `quickrs.exe`.
3. Windows may warn about an unknown app, because the programs aren't signed. If it does, click *More info*, then
   *Run anyway*.
4. Wait for your song list. quickRS looks for your Rocksmith songs by itself, and lists the game's own songs and
   your song packs too.
5. Did you open quickRS before you made `keys.json`? Then press **F5** now. (Or click *Rescan* on the bar at the
   bottom of the song list.) quickRS reads all your songs again. With a big library this can take up to a minute.

**If you don't see your songs:**
1. In File Explorer, open Rocksmith's game folder, `Rocksmith2014` (with Steam it's usually
   `C:\Program Files (x86)\Steam\steamapps\common\Rocksmith2014`). You'll see the `dlc` folder in it.
2. Drag the `dlc` folder onto the quickRS window. quickRS remembers it.

*Settings* (F2) → *Library* lists the folders quickRS reads.

**For a Desktop shortcut:** in the quickRS folder, right-click `make_shortcut.ps1` and choose *Run with PowerShell*.

### Step 4: Check your songs open

1. Look at the songs on the first screen of your list.
2. Look under each song's name. **needs keys** in red means that song can't be opened yet: the keys aren't working.
   Those songs also show **?** instead of a tuning, and **0:00** as their length.
   - If you see *needs keys*, go to [If something's not right](#if-somethings-not-right) below.
   - If you don't, you're done. Pick a song and play.

![The player in the dark theme](screenshots/player-dark.png)

## The converter: psarc → feedpak

**This is optional.** You don't need it to play your songs.

**Feedpaks** are an open format for the same songs: one plain, unencrypted file per song, with the song's parts,
audio and lyrics. Once converted, they play **without keys**. Convert if you want to play without the keys, or if
you want your songs in the open format. Your .psarc files are only read, never changed, so Rocksmith keeps working
as before.

The converter needs the keys too, to read your .psarc files, so do step 2 first.

<img src="screenshots/converter.png" width="560" alt="quickRS Converter">

**How to use it:**

1. In the quickRS folder, double-click **`quickRS Converter.exe`**.
2. Look at the top line. It should say **Keys found: ready to convert**. If it says *keys.json is missing*, go to
   [Check the keys](#check-the-keys).
3. Look at box 1, **"Your Rocksmith songs"**. It's filled in by itself only if Rocksmith is in Steam's usual place,
   `C:\Program Files (x86)\Steam\steamapps\common\Rocksmith2014\dlc`. If it's empty, press *Browse…* next to it
   and pick your `dlc` folder.
4. Look at box 2, **"Where the feedpaks go"**. Once box 1 is filled, it shows a folder called `feedpak`, next to
   `dlc`. **Leave it as it is.** quickRS then finds your feedpaks by itself. (If you pick another folder, drag that
   folder onto the quickRS window once after converting.)
5. Press **Convert**. Each song shows in the list as it's done.
6. Wait until the line next to *Convert* starts with *Done*.
7. To see the feedpaks, press *Open the feedpak folder*.

It converts every song in the folder, including those in subfolders. Run it again after buying or adding songs:
only the new ones are converted, and the rest are counted as *already there*.

If you have a song as both a .psarc and a feedpak, quickRS lists it once. To choose which copy plays, open
*Settings* (F2) → *Library* → *Songs in both formats*, and pick *The psarc* or *The feedpak*.

### Converting without the window: Convert songs.bat

`Convert songs.bat` does the same conversion in a console window (a black window with text).

1. Make sure `keys.json` is in the quickRS folder (step 2). Without it, `Convert songs.bat` says *keys.json is
   missing* and stops.
2. In the quickRS folder, double-click `Convert songs.bat`.
3. It asks *Which folder has your .psarc songs?*
   - If it says *Press Enter for Rocksmith's own: …*, press Enter.
   - If not, type or paste the full path of your `dlc` folder, then press Enter.
4. It asks *Where should the feedpaks go?* and suggests a folder. Press Enter to accept it.
5. Wait until it says *Feedpaks are in: …*.
6. Press any key to close the window.

**From a command prompt**, in the quickRS folder, the same conversion is:

```bat
quickrs-cli.exe convert-folder "C:\...\Rocksmith2014\dlc" "C:\...\Rocksmith2014\feedpak"
```

## If something's not right

**Songs say *needs keys* (in red).**
1. Check that `keys.json` is in the quickRS folder, next to `quickrs.exe`.
2. Close quickRS.
3. Open quickRS again.
4. Press **F5**. (F5 alone, without closing quickRS first, doesn't pick up new keys.)
5. Still *needs keys*? Go to [Check the keys](#check-the-keys).

The hint quickRS shows for such a song points to *Settings › Library*, but the keys aren't there. They only go in
`keys.json`, in the quickRS folder.

#### Check the keys

quickRS itself shows no error for a wrong `keys.json`: the songs just keep saying *needs keys*. The converter
is the place to check.

1. In the quickRS folder, double-click `quickRS Converter.exe`.
2. Read its top line:
   - **Keys found: ready to convert**: the keys work. Close quickRS, open it again, and press F5.
   - **keys.json is missing**: either there's no `keys.json`, or it's there but something's wrong with it. The
     converter doesn't say which. Check the three things below.

The three things to check:

- **The name.** It may have been saved as `keys.json.txt`, and Windows may hide the `.txt`, so the name looks
  right. Open the file in Notepad, then do steps 10 to 14 of [step 2](#step-2-add-rocksmiths-two-keys) again,
  typing `"keys.json"` with the quotes.
- **Example text left in.** Open `keys.json` in Notepad. There must be no `<` or `>` left in it.
- **Keys pasted wrongly.** Each key must be whole (64 characters, or the `0x1A, 0x2B, …` form), on one line,
  between its two quotes. The quotes, the colons and the comma must still be there.

After fixing it, press *Check again* in the converter. When it says *Keys found*, close quickRS, open it again,
and press F5.

**No songs at all.** Drag your `dlc` folder onto the quickRS window: see *If you don't see your songs* in
[step 3](#step-3-start-quickrs).

**Windows warns about an unknown app.** Click *More info*, then *Run anyway*.

**The converter's "Your Rocksmith songs" box is empty.** Press *Browse…* and pick your `dlc` folder.

**Convert songs.bat doesn't offer your songs folder when it asks.** Type or paste the full path of your `dlc`
folder, then press Enter.

---

## What it does

quickRS reads the songs you already have and turns them into a fast, clean practice tool. It runs next to your
Rocksmith install and never changes it.

### 🎸 Play along to real tab
The tab scrolls with the song and highlights the bar you're in, with the next notes ringed. The Lead, Rhythm and
Bass parts are one click apart, and the rhythm notation is drawn under the staff. Next to the tab are the **lyrics**,
a **tuner** set to the song's tuning, and the **note panel**.

![Snow (Hey Oh) with the note panel on the guitar neck](screenshots/neck-dark.png)

**The note panel** shows what's coming next: a chord on a piano, with its shape and fingering, or a single note on
the guitar neck, with the string and fret. It also shows the note it hears you play ("you: Eb3 +3¢").

<img src="screenshots/zoom_note_panel-light.png" width="700" alt="The note panel: a chord on the piano, a note on the neck">

Move the tiles around, or turn off the ones you don't need. Here the tuner is at the top:

![The tuner, score and lyrics tiles](screenshots/tuner_lyrics-dark.png)

### 🔁 Loop it, slow it down, level up
**Loop a section:** `[` and `]` set the loop's start and end (snapped to the beat, with a short lead-in each time
round), and `L` loops the section you're in. The loop shows on the tab and on the seek bar:

<img src="screenshots/zoom_loop-dark.png" width="760" alt="A loop from 0:36 to 0:48, on the tab and the seek bar">

**Hear a passage on its own:** Ctrl+drag across some bars (or Ctrl+click a bar) to select them. A bar of keys pops
up under the selection: **Play** (`P`) plays it once, **Loop** repeats it, and **Slow** (`S`) takes it to 70%, then
50%, without changing the pitch. The key on the left chooses what you hear:

- **The synth guitar** (the wave): only the notes in the tab, played on a synthesised guitar, so you hear exactly
  what's written, even at 50% speed.
- **The song** (the bars): the real recording of that passage, with the band.

<img src="screenshots/zoom_synth_or_song-dark.png" width="760" alt="A selection played on the synth guitar at 70% speed, and the same selection playing the song">

![Monkey Wrench with a loop over three bars](screenshots/loop-light.png)

**Slow the whole song down** with the speed knob (25% to 150%, the pitch stays the same). **Jump to any section**
from the list over the seek bar, or with PgUp and PgDn:

![Strawberry Fields Forever with the section list open](screenshots/sections-light.png)

**Practice controls** (the *Controls* key) have it all in one place: speed, difficulty, the loop keys, and
*Level up after a clean loop*, which steps the difficulty up each time you nail the loop.

![Practice controls](screenshots/controls-light.png)

### 🎯 Scored as you play
Your guitar is listened to as you play. Every note counts as hit, late, early or missed, and your best runs are
kept for each part.

![Results for Heat of the Moment](screenshots/results-dark.png)

### 📚 Your whole library, straight away
quickRS looks for your songs by itself, in your Steam libraries. You get covers, search, sorting, favourites,
playlists, a tuning filter, and a preview of the song you've selected. Home picks up where you left off.

![Home](screenshots/00_home-light.png)

![The library](screenshots/02_library_selected-dark.png)

![Picking a part](screenshots/03_picker-light.png)

### 🎛️ Your tone, your look
Built-in guitar tones (As is, Clean, Crunch, Lead, Bass, Acoustic), or make your own with the drive, EQ and
effects knobs, and save it. Three themes: Midnight and Graphite (dark) and Daylight (light). Four knob styles.

![Guitar tone](screenshots/20_settings_tone-dark.png)

### 🔌 Your guitar, your way
- **Rocksmith Real Tone Cable:** plug it in and it's picked, as in Rocksmith.
- **Audio interfaces and amp modellers:** WASAPI (with a low-latency exclusive mode) or ASIO, and your RS_ASIO
  setup works too.
- **A microphone**, for an acoustic guitar.

It follows devices as you plug them in and out, and finds your guitar's channel from the first notes you play.

---

## Build it yourself

The whole program is here as Rust source (MIT), the same code as the release.

| Folder | What's in it |
|---|---|
| [`quickRS/`](quickRS) | the program, built and ready to run (the release zip's contents) |
| `crates/qrs-app` | the player, `quickrs.exe` |
| `crates/qrs-convert` | the converter window, `quickRS Converter.exe` |
| `crates/qrs-cli` | the command-line tools, `quickrs-cli.exe` (converter, song info, tabs, devices) |
| `crates/qrs-formats` | Rocksmith 2014 psarc / sng / xml and feedpak, the chart model, tab rendering |
| `crates/qrs-audio` | the audio engine: WASAPI and ASIO devices, tones, decoding, slow-down, the synth |
| `crates/qrs-analysis` | pitch, onset and note detection, scoring |
| `crates/qrs-library` | the library scan, settings and user data, the feedpak exporter |
| `crates/qrs-ui` | the look: themes, fonts, components |
| `share/` | the release's readme, `Convert songs.bat`, and `package.ps1`, which makes the release zip |

**You need** (Windows 10 or 11, 64-bit):
- [Rust](https://rustup.rs) (stable) with the MSVC toolchain, and the *Visual Studio Build Tools* with
  *Desktop development with C++*.
- [LLVM](https://github.com/llvm/llvm-project/releases), for the ASIO support: set `LIBCLANG_PATH` to its `bin`
  folder. The ASIO SDK is downloaded by the build.

**Build and run:**

```bat
cargo build --release -p qrs-app -p qrs-cli -p qrs-convert
cargo run --release -p qrs-app
cargo test --workspace
powershell -ExecutionPolicy Bypass -File share\package.ps1
```

The programs land in `target/release`; `package.ps1` makes the release zip in `dist\share`. For your songs,
put `keys.json` in the folder you run from (it's in `.gitignore`), as in
[step 2](#step-2-add-rocksmiths-two-keys) above.

---

## Keyboard shortcuts

| | |
|---|---|
| `Space` | play / pause |
| `[` `]` `L` | loop start / end / this section |
| `↑` `↓` | zoom the tab |
| `F2` | settings |
| `F11` | full screen |
| `Esc` | back |

Every page shows its keys on its buttons.

---

## ☕ Support it

quickRS is free, and it'll stay free. But vibecoding isn't free! The AI that wrote most of this code runs on
credits. If quickRS helps your practice, you can buy me a coffee:

### [**ko-fi.com/arpanmitra**](https://ko-fi.com/arpanmitra)

---

<sub>quickRS is MIT licensed. The Manrope font is SIL OFL, and ww2ogg's codebooks are BSD. quickRS isn't made by or
connected with Ubisoft. Rocksmith is a trademark of Ubisoft Entertainment. Bring your own songs.</sub>
