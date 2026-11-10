quickRS for Linux: early test build
===================================

You need: a 64-bit PC with a reasonably current distro (Ubuntu 22.04 or newer, Mint 21+, Debian 12+, Fedora 36+,
current Arch, and their relatives); a normal desktop (GNOME, KDE, Cinnamon, Xfce...) with PipeWire or
PulseAudio, which nearly every distro has; your guitar on an audio interface (or a Real Tone Cable, or a
microphone); and your own RS2014 songs. quickRS comes with no songs and no RS keys.


1. Run it
---------
    chmod +x quickRS-*-linux-x86_64.AppImage
    ./quickRS-*-linux-x86_64.AppImage
(or in your file manager: Properties > Permissions > "Allow executing as program", then double-click it).

If it doesn't start:
  - "libasound.so.2: cannot open shared object file": install ALSA's library (every desktop has it; a minimal
    install may not):  Ubuntu/Debian: sudo apt install libasound2t64   (older: libasound2)
                       Fedora: sudo dnf install alsa-lib    Arch: sudo pacman -S alsa-lib
  - a FUSE message and nothing else: run it as
        ./quickRS-*-linux-x86_64.AppImage --appimage-extract-and-run
    or use the .tar.gz instead: unpack it and run ./quickrs in its folder.
  - anything else: run it from a terminal and send what it prints, and the log (section 7).

The same file also holds quickRS's two helpers:
    ./quickRS-*.AppImage convert        the song converter (a window)
    ./quickRS-*.AppImage cli devices    what audio devices quickRS sees, and what it picks (useful in a report)


2. Your songs (from RS on Windows)
-----------------------------------------
quickRS plays RS's .psarc files (and feedpaks) straight from a folder. On Windows they are in
    C:\Program Files (x86)\Steam\steamapps\common\Rocksmith2014\dlc
(songs.psarc next to it, in the Rocksmith2014 folder, holds the on-disc songs). Copy the dlc folder to your Linux
home, for example ~/RS/dlc. A Windows drive that Linux can see works too: point quickRS at it there.

With Steam on Linux and RS installed through it (Proton), quickRS finds the dlc folder by itself.

Then in quickRS: the welcome's "Pick a folder..." or Settings (F2) > Directories > "Add folder...". If no folder
dialog appears (some minimal desktops have no file-picker service), quickRS says so: drag the folder from your
file manager onto quickRS's window instead, or install xdg-desktop-portal-gtk (or xdg-desktop-portal-kde on KDE).


3. RS's keys (only for .psarc files)
-------------------------------------------
.psarc files are encrypted. quickRS needs the "PSARC key" and the "SNG key for PC": each is 64 characters of 0-9
and A-F, the same in every copy of RS; they are not a licence key. quickRS can't include them; they're
published in open-source RS modding tools.

  1. Settings (F2) > Directories, scroll down to RS KEYS.
  2. Paste the PSARC key and the SNG key (or a whole keys.json text into either box), press "Save keys".

Or convert your songs once to feedpaks (the converter, section 1), which play without keys.


4. Plug in your guitar
----------------------
quickRS takes the guitar from your desktop's sound system (PipeWire or PulseAudio), the same as any other app:

  1. In your desktop's Sound settings, pick your audio interface as the INPUT device.
  2. In quickRS, Settings (F2) > Audio input / output: "System default input" should be picked.
     Your interface may also be listed as "<its name> (direct)": that takes it straight from the hardware, which
     can be quicker, but only while nothing else uses it. If it says the device is busy, go back to the system
     default.
  3. With a 2-input interface (Scarlett Solo and the like) the guitar goes in the instrument jack with INST on;
     quickRS finds the right channel from the signal, so strum a little.
  4. Calibrate (it's offered at the first start): guitar volume and tone knobs all the way up, hands off for 2
     seconds, then strum as hard as you can until the bar fills.

INPUT CHANNEL offers every channel the input reports, not just the first four. Extra choices wrap into rows
in Settings and the guided setup. Auto and individual channel meters cover the first 16 channels; higher
channels can be picked by hand. Use the main input level meter to check them; their individual meters stay dark.

Delay between your strum and what you hear: PipeWire chooses its own buffer size. To try a smaller one while you
play (until the next restart), in a terminal:
    pw-metadata -n settings 0 clock.force-quantum 128
and to undo it:
    pw-metadata -n settings 0 clock.force-quantum 0
Please tell me whether this made a difference (section 8).


5. The first start
------------------
A short welcome shows three steps: your song folder, the keys, your guitar input. "Set up your guitar?" then walks
you through input, output, a level check and calibration. Settings (F2) > Audio input / output > "Set up your
guitar" runs it again.


6. Updates
----------
quickRS looks for a new version at most once a day. On Linux "Update now" opens the download page: download the
new AppImage and replace the old file. Settings (F2) > Updates turns the check off.


7. Your data, and the log
-------------------------
Settings, scores, playlists and keys:  ~/.local/share/quickrs/
The log:                               ~/.local/share/quickrs/.quickrs-cache/quickrs.log
(Settings (F2) > Updates > "Show the log file" shows it; "Report a bug" there opens a GitHub issue with the
version and your system filled in.) Deleting ~/.local/share/quickrs starts quickRS afresh.


8. What to report
-----------------
Anything that's wrong, and also what works. Most useful:
  - your distro and desktop, and your audio interface;
  - whether quickRS started, and whether your guitar was heard (and on which input);
  - how the timing feels when you play along: fine, a little late, unplayable? With the pw-metadata tip above?
  - anything that looks wrong on screen;
  - the log (section 7), and the output of   ./quickRS-*.AppImage cli devices

Send it to the person who gave you this build, or open an issue on
    https://github.com/arpan2711/quickRS-player/issues
