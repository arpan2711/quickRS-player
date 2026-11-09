Troubleshooting
---------------
Most of these came from people trying 0.1.0. If your problem isn't here,
please [report it](#reporting-a-bug).

* [Reporting a bug](#reporting-a-bug)
* [Songs say "needs keys"](#songs-say-needs-keys)
* [No songs at all](#no-songs-at-all)
* [Some songs are missing](#some-songs-are-missing)
* [A song is listed twice, or the wrong copy plays](#a-song-is-listed-twice-or-the-wrong-copy-plays)
* [A feedpak plays only one instrument](#a-feedpak-plays-only-one-instrument)
* [Notes don't count](#notes-dont-count)
* [Chords or strums don't count](#chords-or-strums-dont-count)
* [A recording isn't in the folder](#a-recording-isnt-in-the-folder)
* [Everything is late or early](#everything-is-late-or-early)
* [Clicks or crackles](#clicks-or-crackles)
* [The update didn't install](#the-update-didnt-install)
* [A song needs the Mac SNG key](#a-song-needs-the-mac-sng-key)
* [quickRS closed when I switched my interface on](#quickrs-closed-when-i-switched-my-interface-on)
* [The screen goes dark during a song](#the-screen-goes-dark-during-a-song)
* [Windows or macOS won't open it](#windows-or-macos-wont-open-it)
* [The window flashes and closes](#the-window-flashes-and-closes)


### Reporting a bug

In quickRS: Settings (`F2`) > *Updates* > *REPORT A BUG*.

* *Report a bug* opens a new issue on
  [GitHub Issues](https://github.com/arpan2711/quickRS-player/issues) in your
  browser, with your quickRS version and system already filled in, and a
  short form: what happened, what you did just before, what you expected, and
  your audio setup (interface, amp modeller, Real Tone Cable or mic).
* *Show the log file* opens the folder with `quickrs.log` selected. If quickRS
  crashed or showed an error, drag the log into the issue.

Nothing is sent by quickRS itself: you see the issue before you post it. You
need a (free) GitHub account. If quickRS won't start at all, open an issue on
the [Issues page](https://github.com/arpan2711/quickRS-player/issues/new)
yourself; the log is `.quickrs-cache\quickrs.log` in the
[data folder](INSTALL.md#where-your-data-lives).


### Songs say "needs keys"

The psarc songs can't be read because quickRS has no keys, or the keys are
wrong.

1. Open Settings (`F2`) > *Library* and look at *RS KEYS*. It says
   whether keys were found, which file they came from or where it looked, and
   what is wrong with a file that doesn't read.
2. Paste the two keys into their boxes and press *Save keys*. The waiting
   songs are read again straight away. See
   [GETTING-SONGS](GETTING-SONGS.md#paste-them-in-settings).
3. If you use `keys.json` instead: Notepad likes to save it as
   `keys.json.txt`. quickRS reads that name too, but check the keys inside:
   no example text left in, each key 64 characters, quotes and commas in
   place.
4. The converter (`quickRS Converter.exe`) checks the keys too. Its top line
   says whether they work and, if not, what's wrong.

In 0.1.0 you had to restart quickRS and press `F5` after adding keys. Since
0.3.0 you don't.

If you'd rather not deal with keys at all, convert your songs to feedpaks once
(the converter needs the keys for that one run) and turn off *Search
Rocksmith's dlc folder*. See [GETTING-SONGS](GETTING-SONGS.md#song-folders).


### No songs at all

quickRS didn't find your `dlc` folder. That happens when Rocksmith isn't in a
Steam library, or Steam is somewhere unusual.

Settings > *Library* > *Add folder...*, and pick the folder with your `.psarc`
or `.feedpak` files. Or drag that folder onto the quickRS window.


### Some songs are missing

* Press `F5` in the library to read the folders again. New files are picked
  up.
* Check the folder is listed in Settings > *Library* > *SONG FOLDERS*.
  Subfolders are read, so adding the top folder is enough.
* If you turned off *Search Rocksmith's dlc folder*, the psarcs in Rocksmith's
  `dlc` folder (and `songs.psarc`) are skipped on purpose.
* A pack with several songs is listed song by song, so search by song title,
  not pack name.


### A song is listed twice, or the wrong copy plays

A song you have as both a psarc and a feedpak is listed once and marked
`feedpak + psarc` (the first one named plays). Settings > *Library* > *SONGS
IN BOTH FORMATS* picks which copy plays.

If it's listed twice, the two copies differ (title, artist or length), so
quickRS treats them as different songs.


### A feedpak plays only one instrument

0.1.0 played one stem of a feedpak with stems as if it were the whole song.
0.3.0 plays all the stems. If one is silent, check *Controls* > *PITCH &
STEMS*: it may be off, turned down, or another stem may be soloed. *Stems as
the pack sets them* puts the mix back.


### Notes don't count

* Run *Set up my guitar* (or *Set up my bass*) in Settings > *Sound*. It checks
  the input, the level and the open strings, and calibrates. See
  [AUDIO-SETUP](AUDIO-SETUP.md#the-guided-setup).
* Or check the input by hand: the meter by the guitar input in Settings >
  *Sound* should move when you play.
* Calibrate it: Settings > *Sound* > *INPUT LEVEL* >
  *Check level*. See [AUDIO-SETUP](AUDIO-SETUP.md#calibrating-your-input).
* On an interface, check the channel: *INPUT CHANNEL*, or let quickRS find it
  by playing a few clear notes.
* Tune up. The tuner tile targets the song's tuning and follows any guitar
  input transpose.
* If you shifted the guitar input, quickRS scores and displays the shifted
  notes. *No shift* in *Controls* > *PITCH* puts it back; the backing track
  does not change.


### Chords or strums don't count

0.3.0 scores chords more like Rocksmith (a chord counts when at least half its
notes sound) and finds strums through a distorted or clipping input much
better than 0.1.0 did. If rhythm parts still score low:

* Turn the gain on your interface down until its light stays off on your
  hardest strum, and calibrate again.
* On an amp modeller, a clean or lightly driven sound is easier to score
  than heavy distortion. With a dry input, the pedalboard's distortion
  doesn't matter: quickRS listens to the guitar before it.
* A take (`Ctrl+R`) shows which chords it heard wrong and what it heard
  instead.


### A recording isn't in the folder

A recording appears in your recordings folder when you stop it (`Shift+R`),
or when you leave the song or the amp room. Until then it's written in
quickRS's own cache folder, so a recording that's running or paused isn't in
the folder yet. A recording under a second long isn't kept. If quickRS closed
mid-recording, the next start puts what was recorded in the folder, marked
*(recovered)*.

Recordings from a song start with the song's name
(`<song> - quickRS <date> <time>.wav`). Settings > *Library* >
*RECORDINGS* says where they go and has *Open*. See
[PLAYING](PLAYING.md#recording).

(`R` records what you hear. A take, the loop checked against the chart, is
`Ctrl+R`.)


### Everything is late or early

If all your hits show orange (early or late), move *Guitar timing* with
`Ctrl+,` and `Ctrl+.` until they turn green. If the tab is out of step with the
music, move *Audio / notes sync* with `,` and `.`.


### Clicks or crackles

Make the buffer bigger: Settings > *Sound* > *Buffer*. On Windows, if your
interface has an ASIO driver, try it; otherwise try turning *Low-latency
output* off.


### The update didn't install

* If the release page opened in your browser, quickRS's folder can't be
  written to (for example it is in `C:\Program Files`, or on a Mac the app
  runs from the .dmg or Downloads). Move quickRS somewhere you can write to,
  or update [by hand](UPDATING.md#by-hand).
* "Couldn't update" with a reason: nothing was changed. Try again later, or
  update by hand.
* No notice at all: check *Check for updates on start* in Settings >
  *Updates*, or press *Check for updates* there. A version you skipped shows
  there with *Offer it again*.


### A song needs the Mac SNG key

Songs from RS's Mac edition (files ending `_m.psarc`) are encrypted with a
third key. Open Settings (`F2`) > *Library* > *RS KEYS*, paste it into *Mac
SNG key* and press *Save keys*; your other keys stay as they are. See
[GETTING-SONGS](GETTING-SONGS.md#paste-them-in-settings).


### quickRS closed when I switched my interface on

Before 0.5.6, an interface whose ASIO driver crashes (a Line 6 HX Stomp's,
#1) closed quickRS as soon as Windows saw it, whatever device was chosen.
Since 0.5.6 that driver is left out and Settings > *Sound* > *All devices*
says so; pick the interface's WASAPI entry, and check the maker's site for a
newer driver.


### The screen goes dark during a song

Since 0.5.6 quickRS keeps the screen on while a song plays or a recording
runs, on Windows and macOS, and lets it sleep again when you pause. On Linux
it doesn't yet: set the screen's blank time longer in your desktop's settings.


### Windows or macOS won't open it

Neither build is notarised or signed with a paid certificate. On Windows click
*More info*, then *Run anyway*. On macOS see [INSTALL](INSTALL.md#macos).


### The window flashes and closes

quickRS 0.1.0 drew with Vulkan, and some Intel graphics drivers crash in
their Vulkan driver (`igvk64.dll` in Windows' Event Viewer) before the window
appears. Since 0.3.0 quickRS draws with DirectX 12 on Windows. If a start still closes
before the window shows, the next start tries OpenGL, then Vulkan, by itself:
open it two or three times.

To pick one by hand, start it from a command prompt in quickRS's folder with
`set QUICKRS_GPU=gl` (or `dx12`, `vulkan`) and then `quickrs`. Updating the
graphics driver from Intel's site also helps.
