quickRS Bristol 2026
====================

A song library and player for your Rocksmith 2014 songs, played with your real guitar. The tab scrolls with the
song and your notes are scored as you play. You can loop a passage and slow it down without changing its pitch.
It plays .psarc files straight from Rocksmith's folder. It also plays feedpaks, an open format for the same
songs (one file per song, no encryption), which quickRS can make from your .psarc files.

Windows 10 or 11, 64-bit. There's no installer: unzip the folder anywhere and run quickrs.exe.
The first time, Windows may warn about an unknown app because the program isn't signed. Click "More info",
then "Run anyway".

quickRS comes with no songs and no Rocksmith files. You need your own.


1. Keys
-------
Rocksmith's .psarc files are encrypted. quickRS doesn't ship the keys, but they're published in open-source
Rocksmith modding tools.
Copy keys.example.json to keys.json (in this folder), then paste in the two keys, 64 hex characters each:
    { "psarc_key": "...", "sng_key": "..." }
With keys.json in place you can play .psarc files directly and convert them. Feedpaks don't need keys.


2. Plug in your guitar
----------------------
The Rocksmith Real Tone Cable works as it does in Rocksmith: plug it in and quickRS picks it.
Other inputs work too: an audio interface or amp modeller (including its ASIO driver, or your RS_ASIO setup),
or a microphone for an acoustic guitar.
Settings (F2) > Audio shows what's picked and follows devices as you plug them in and out. Pick a device there
to fix it. The guitar's channel is found from the first clear notes you play.


3. Start
--------
Double-click quickrs.exe. For a Desktop shortcut, right-click make_shortcut.ps1 and pick
"Run with PowerShell".
On the first start, quickRS finds Rocksmith's song folder (...\Rocksmith2014\dlc) in every Steam library and
lists the songs there.
If your songs are somewhere else, drag the folder onto the quickRS window and it stays in your library.
Settings > Library lists your folders, and F5 reads them again.


4. Convert your songs to feedpaks (optional)
--------------------------------------------
Feedpaks need no keys, and they're plain zip files you can look inside.

The easy way: double-click "quickRS Converter.exe".
  1. Check it says "Keys found". If it doesn't, add keys.json first (step 1) and press "Check again".
  2. Your Rocksmith songs: it fills in Rocksmith's dlc folder if it finds it. If not, press Browse and pick the
     folder with your .psarc files.
  3. Where the feedpaks go: it fills in a "feedpak" folder next to "dlc". Leave it, and quickRS finds the
     feedpaks by itself.
  4. Press Convert. Each song shows as it's done; then "Open the feedpak folder" shows you the result.
It converts every song, including those in subfolders and every song in a pack like songs.psarc. Run it again
after adding songs: the ones already converted are skipped.

No window? "Convert songs.bat" does the same in a console: double-click it and press Enter twice.

From a command prompt, the same:
    quickrs-cli.exe convert-folder "C:\...\Rocksmith2014\dlc" "C:\...\Rocksmith2014\feedpak"
or one song:
    quickrs-cli.exe feedpak "C:\...\dlc\somesong_p.psarc" -o "Some Song.feedpak"
Add --dir to write each song as a folder instead of a zip. Add --force to convert again songs that are already
there.

Setting up the feedpak folder:
  - Put it next to Rocksmith's dlc folder and call it "feedpak" (the converter's default). quickRS then finds
    it by itself on every start.
  - Anywhere else: drag the folder onto the quickRS window once.
A song you have as both a .psarc and a feedpak shows once. Settings > Library picks which copy plays.
Your .psarc files are only read, never changed, so Rocksmith keeps working as before.


Keys in the app
---------------
F2 settings, F11 full screen, Space play / pause, Esc back. Every page shows its keys on its buttons.

Your data
---------
quickRS keeps these in its data folder:
  - config.json: your settings and song folders.
  - userdata.json: your scores, favourites and playlists.
  - .quickrs-cache: the library index and covers.
The data folder is the first of these that applies:
  - the folder named in the QUICKRS_DATA environment variable;
  - this folder, if a config.json or userdata.json is here (a portable install);
  - otherwise %APPDATA%\quickRS.

Like it? quickRS is free, but vibecoding isn't free! Buy me a coffee: https://ko-fi.com/arpanmitra

Command-line tools: run quickrs-cli.exe --help (song info, tabs, audio, feedpak conversion, devices, library).
quickrs.exe --silent starts without any sound device.
Licences are in the licences folder: quickRS is MIT, the Manrope font is SIL OFL, and ww2ogg's codebooks are
BSD.
quickRS isn't made by or connected with Ubisoft. Rocksmith is a trademark of Ubisoft Entertainment.
