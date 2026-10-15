quickRS Bristol 2026
====================

A song library and player for your Rocksmith 2014 songs, played with your real guitar. The tab scrolls with the
song and your notes are scored as you play. You can loop a passage and slow it down without changing its pitch.

You do NOT need to convert anything. quickRS plays your Rocksmith songs as they are, straight from Rocksmith's own
folder. It only needs Rocksmith's two keys, in a file called keys.json next to quickrs.exe. You make that file
once, in step 1. Converting to feedpaks (step 4) is optional.

You need: Windows 10 or 11 (64-bit), your guitar (a Real Tone Cable, an audio interface, or a mic), and your own
Rocksmith 2014 songs. quickRS comes with no songs and no Rocksmith files.

There's no installer: this folder, the one with quickrs.exe in it, is quickRS. Below it's called "the quickRS
folder". You can move the whole folder anywhere you like.

Words used below:
  - .psarc file: the kind of file Rocksmith keeps its songs in. Most hold one song. Some hold many: the game's own
    songs.psarc has its 70 songs, and song packs hold several.
  - dlc folder: the folder where Rocksmith keeps your .psarc files. It's called "dlc", and it's inside
    Rocksmith's game folder, "Rocksmith2014". With Steam it's usually
    C:\Program Files (x86)\Steam\steamapps\common\Rocksmith2014\dlc
  - keys: two codes that unlock .psarc files. Step 1 explains them.
  - F5, F2: keys on the top row of your keyboard. On some laptops you hold Fn while you press them.
  - File name endings (.exe, .json, .txt): Windows often hides them. So quickrs.exe may show as just "quickrs",
    and keys.example.json as "keys.example". That's normal.


1. Add Rocksmith's two keys
---------------------------
Rocksmith's .psarc files are locked (encrypted). quickRS needs two keys to open them. About the keys:
  - Their names are the "PSARC key" and the "SNG key for PC".
  - Each one is a code 64 characters long, made only of the digits 0-9 and the letters A-F.
  - They're the same in every copy of Rocksmith. They are NOT a licence key, and they have nothing to do with
    your Steam account.
  - quickRS can't include them. They're published in open-source Rocksmith modding tools.

Find both keys first. Then make keys.json in the quickRS folder, next to quickrs.exe:

  1. In the quickRS folder, right-click keys.example.json and choose Open with > Notepad.
  2. Look at what's inside. It's two lines between { and }, like this (your example text is different):
         {
           "psarc_key": "<example text>",
           "sng_key": "<example text>"
         }
  3. Copy the PSARC key from where you found it: select it and press Ctrl+C.
  4. In Notepad, on the "psarc_key" line, select the example text, from < to >, both included. Don't select the
     quotes around it.
  5. Press Ctrl+V. The key takes the place of the example text.
  6. Copy the SNG key for PC the same way: select it and press Ctrl+C.
  7. In Notepad, on the "sng_key" line, select the example text, from < to >.
  8. Press Ctrl+V.
  9. Check both lines. Each key sits between its two quotes, on one line. There's no < or > left. The quotes,
     the colons, and the comma at the end of the first line are still there. (A key written as 0x1A, 0x2B, ...
     works too.)
  10. Click File > Save as.
  11. Check that the Save window is in the quickRS folder: its name (for example quickRS-0.1.0) shows at the
      top. If it isn't, go to the quickRS folder.
  12. In the "File name" box, delete what's there and type "keys.json" WITH the quotes. The quotes stop
      Windows from adding .txt to the name.
  13. Click Save.
  14. Close Notepad.

keys.json is now in the quickRS folder, next to quickrs.exe. Leave keys.example.json where it is.


2. Plug in your guitar
----------------------
The Rocksmith Real Tone Cable works as it does in Rocksmith: plug it in and quickRS picks it.
Other inputs work too: an audio interface or amp modeller (including its ASIO driver, or your RS_ASIO setup),
or a microphone for an acoustic guitar.
Settings (F2) > Audio shows what's picked and follows devices as you plug them in and out. Pick a device there
to fix it. The guitar's channel is found from the first clear notes you play.


3. Start quickRS
----------------

  +----------------------------------------------------------------------------+
  |  Already opened quickRS before adding the keys?                            |
  |  Close it, open it again, then press F5.                                   |
  +----------------------------------------------------------------------------+

  quickRS reads keys.json only when it starts. Songs it saw without the keys stay marked "needs keys", even
  after you open it again, until you press F5. F5 makes quickRS read your song folders again. F5 in a quickRS
  window that was open while you made keys.json doesn't help: close it and open it first.
  Never opened quickRS before making keys.json? Then you don't need F5.

  1. If quickRS is open, close it.
  2. In the quickRS folder, double-click quickrs.exe.
  3. Windows may warn about an unknown app, because the programs aren't signed. If it does, click
     "More info", then "Run anyway".
  4. Wait for your song list. quickRS looks for your Rocksmith songs by itself, and lists the game's own songs
     and your song packs too.
  5. Did you open quickRS before you made keys.json? Then press F5 now. (Or click "Rescan" on the bar at the
     bottom of the song list.) quickRS reads all your songs again. With a big library this can take up to a
     minute.
  6. Look at the songs on the first screen of your list, under each song's name. "needs keys" in red means
     that song can't be opened yet: the keys aren't working. Those songs also show "?" instead of a tuning,
     and 0:00 as their length. If you see it, go to "If something's not right" below.
     If you don't, you're done. Pick a song and play.

If you don't see your songs:
  1. In File Explorer, open Rocksmith's game folder, Rocksmith2014 (with Steam it's usually
     C:\Program Files (x86)\Steam\steamapps\common\Rocksmith2014). You'll see the dlc folder in it.
  2. Drag the dlc folder onto the quickRS window. quickRS remembers it.
  Settings (F2) > Library lists the folders quickRS reads.

For a Desktop shortcut: in the quickRS folder, right-click make_shortcut.ps1 and choose "Run with PowerShell".


4. Convert your songs to feedpaks (optional)
--------------------------------------------
You don't need this to play your songs.

Feedpaks are an open format for the same songs: one plain, unencrypted file per song, with the song's parts,
audio and lyrics. Once converted, they play without keys. Convert if you want to play without the keys, or if
you want your songs in the open format. Your .psarc files are only read, never changed, so Rocksmith keeps
working as before.

The converter needs the keys too, to read your .psarc files, so do step 1 first.

The easy way: "quickRS Converter.exe"
  1. In the quickRS folder, double-click "quickRS Converter.exe".
  2. Look at the top line. It should say "Keys found: ready to convert". If it says "keys.json is missing",
     go to "Check the keys" below.
  3. Look at box 1, "Your Rocksmith songs". It's filled in by itself only if Rocksmith is in Steam's usual
     place, C:\Program Files (x86)\Steam\steamapps\common\Rocksmith2014\dlc
     If it's empty, press "Browse..." next to it and pick your dlc folder.
  4. Look at box 2, "Where the feedpaks go". Once box 1 is filled, it shows a folder called "feedpak", next to
     "dlc". Leave it as it is. quickRS then finds your feedpaks by itself. (If you pick another folder, drag
     that folder onto the quickRS window once after converting.)
  5. Press Convert. Each song shows in the list as it's done.
  6. Wait until the line next to Convert starts with "Done".
  7. To see the feedpaks, press "Open the feedpak folder".

It converts every song in the folder, including those in subfolders. Run it again after buying or adding
songs: only the new ones are converted, and the rest are counted as "already there".

If you have a song as both a .psarc and a feedpak, quickRS lists it once. To choose which copy plays, open
Settings (F2) > Library > "Songs in both formats", and pick "The psarc" or "The feedpak".

Without the window: "Convert songs.bat"
It does the same conversion in a console window (a black window with text).
  1. Make sure keys.json is in the quickRS folder (step 1). Without it, "Convert songs.bat" says "keys.json
     is missing" and stops.
  2. In the quickRS folder, double-click "Convert songs.bat".
  3. It asks "Which folder has your .psarc songs?"
       - If it says "Press Enter for Rocksmith's own: ...", press Enter.
       - If not, type or paste the full path of your dlc folder, then press Enter.
  4. It asks "Where should the feedpaks go?" and suggests a folder. Press Enter to accept it.
  5. Wait until it says "Feedpaks are in: ...".
  6. Press any key to close the window.

From a command prompt, in the quickRS folder, the same conversion is:
    quickrs-cli.exe convert-folder "C:\...\Rocksmith2014\dlc" "C:\...\Rocksmith2014\feedpak"


If something's not right
------------------------
Songs say "needs keys" (in red):
  1. Check that keys.json is in the quickRS folder, next to quickrs.exe.
  2. Close quickRS.
  3. Open quickRS again.
  4. Press F5. (F5 alone, without closing quickRS first, doesn't pick up new keys.)
  5. Still "needs keys"? Go to "Check the keys" (below).
  The hint quickRS shows for such a song points to Settings > Library, but the keys aren't there. They only
  go in keys.json, in the quickRS folder.

Check the keys:
  quickRS itself shows no error for a wrong keys.json: the songs just keep saying "needs keys". The converter
  is the place to check.
  1. In the quickRS folder, double-click "quickRS Converter.exe".
  2. Read its top line:
     - "Keys found: ready to convert": the keys work. Close quickRS, open it again, and press F5.
     - "keys.json is missing": either there's no keys.json, or it's there but something's wrong with it. The
       converter doesn't say which. Check the three things below.
  The three things to check:
     - The name. It may have been saved as keys.json.txt, and Windows may hide the .txt, so the name looks
       right. Open the file in Notepad, then do steps 10 to 14 of step 1 again, typing "keys.json" with the
       quotes.
     - Example text left in. Open keys.json in Notepad. There must be no < or > left in it.
     - Keys pasted wrongly. Each key must be whole (64 characters, or the 0x1A, 0x2B, ... form), on one line,
       between its two quotes. The quotes, the colons and the comma must still be there.
  After fixing it, press "Check again" in the converter. When it says "Keys found", close quickRS, open it
  again, and press F5.

No songs at all: drag your dlc folder onto the quickRS window. See "If you don't see your songs" in step 3.

Windows warns about an unknown app: click "More info", then "Run anyway".

The converter's "Your Rocksmith songs" box is empty: press "Browse..." and pick your dlc folder.

"Convert songs.bat" doesn't offer your songs folder when it asks: type or paste the full path of your dlc
  folder, then press Enter.


Keyboard shortcuts
------------------
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
