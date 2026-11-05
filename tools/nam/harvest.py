"""Harvest a personal NAM capture library from TONE3000 for quickRS (owner's build).

For each real unit behind quickRS's RS mapping (crates/qrs-audio/src/board/rs_gear.rs: AMPS, the bass
amps, CABS, effect(); catalog.rs brand()) it searches TONE3000, picks the best-downloaded matching tone and a few
of its models spread over gain, downloads them into the captures folder, checks each with `quickrs-cli nam info`
(IRs with `nam render --ir`), and appends them to captures.json with the fields the picker reads (skin, rs, gain,
bass). Existing items are kept untouched; new ids start at 100.

The API key comes from the environment (T3K_SECRET); it is never written anywhere.

    python tools/nam/harvest.py explore [target-substring]   # searches only: shows what each target would pick
    python tools/nam/harvest.py run [--dry]                  # downloads, verifies, writes captures.json
"""

import hashlib
import json
import os
import re
import subprocess
import sys
import tempfile
import time
import urllib.error
import urllib.parse
import urllib.request
from pathlib import Path

API = "https://www.tone3000.com/api/v1"
REPO = Path(__file__).resolve().parents[2]
CLI = REPO / "target-dev" / "release" / "quickrs-cli.exe"
LIB = Path(os.environ.get("QUICKRS_CAPTURES") or Path(os.environ["APPDATA"]) / "quickRS" / "data" / "captures")
WORK = Path(os.environ.get("T3K_WORK") or Path(tempfile.gettempdir()) / "t3k-harvest")
CACHE = WORK / "cache"
FIRST_ID = 100

# --------------------------------------------------------------------------------------------------------------
# Targets. kind: nam (amp) | pedal | ir. q: search queries (first hit list wins, others add candidates).
# must: regex the tone's title/makes must match. gains: the gain points to spread the picks over (amps/pedals).
# For IRs, mic: regex preferences for the file names (first that matches wins), n files.
# --------------------------------------------------------------------------------------------------------------


def amp(gear, skin, rs, q, must, gains, bass=False, avoid=None, note=""):
    return dict(kind="nam", gear=gear, skin=skin, rs=rs, q=q, must=must, gains=gains, bass=bass, avoid=avoid,
                note=note)


def pedal(gear, skin, rs, q, must, gains, bass=False, avoid=None, note=""):
    return dict(kind="pedal", gear=gear, skin=skin, rs=rs, q=q, must=must, gains=gains, bass=bass, avoid=avoid,
                note=note)


def ir(gear, skin, rs, q, must, n=2, bass=False, avoid=None, note="", file=None):
    return dict(kind="ir", gear=gear, skin=skin, rs=rs, q=q, must=must, n=n, bass=bass, avoid=avoid, note=note,
                file=file)


CLEAN = [1.5, 4.0, 6.0]
FULL = [2.0, 5.5, 8.5]
HIGH = [3.0, 7.0, 9.0]

TARGETS = [
    # ---- guitar amps
    amp("Fender Deluxe Reverb", "amp_clean", ["Amp_AT20"], ["Fender Deluxe Reverb"], r"deluxe reverb", CLEAN,
        avoid=r"hot ?rod|tone ?master"),
    amp("Fender Twin Reverb", "amp_clean", ["Amp_TW26"], ["Fender Twin Reverb"], r"twin", CLEAN,
        avoid=r"tone ?master"),
    amp("Fender Princeton Reverb", "amp_clean", ["Amp_TW26"], ["Fender Princeton Reverb"], r"princeton", CLEAN),
    amp("Fender Hot Rod Deville", "amp_lead", ["Amp_AT120"], ["Fender Hot Rod"], r"hot ?rod", [3.0, 6.0, 8.5],
        note="AT-120 'vintage US high-gain': a channel-switching Fender (closest guess)"),
    amp("Matchless DC-30", "amp_chime", ["Amp_BT15", "Amp_BT30"], ["Matchless DC30", "Matchless"], r"matchless",
        FULL),
    amp("Friedman BE-100", "amp_lead", ["Amp_BT45"], ["Friedman BE100", "Friedman"], r"friedman", HIGH),
    amp("Bogner Ecstasy", "amp_lead", ["Amp_BT45"], ["Bogner Ecstasy", "Bogner"], r"bogner", HIGH,
        avoid=r"uberschall|überschall|shiva|goldfinger"),
    amp("Mesa/Boogie Mark V", "amp_lead", ["Amp_CA38"], ["Mesa Boogie Mark V", "Mesa Boogie Mark IV"],
        r"mark ?(v|iv|5|4)\b", [3.0, 6.0, 8.5],
        note="Cali-38 is a Mark I: TONE3000 has no Mark I captures, so a Mark V/IV (which has a Mark I mode)"),
    amp("Mesa/Boogie Mark IIC+", "amp_lead", ["Amp_CA85"], ["Mesa Boogie Mark IIC+", "Mesa Mark IIC", "Mesa Mark III"],
        r"mark ?(iic|ii c|2c|iii|3)|iic\+", HIGH),
    amp("Mesa/Boogie Dual Rectifier", "amp_modern", ["Amp_CA100"], ["Mesa Dual Rectifier", "Mesa Rectifier"],
        r"dual rec|recto|rectifier", HIGH, avoid=r"badlander|road ?king|roadster"),
    amp("Roland JC-120", "amp_clean", ["Amp_CS90", "Amp_CS100", "Amp_CS120"], ["Roland JC-120", "Roland Jazz Chorus"],
        r"jc[- ]?120|jazz chorus|jc[- ]?40|jc[- ]?22", [1.0, 3.0],
        note="CL-SS 100/120 (Polytone-style, high-watt solid state): the JC stands in"),
    amp("Vox AC30", "amp_chime", ["Amp_EN30"], ["Vox AC30"], r"ac[- ]?30", FULL, avoid=r"ac[- ]?30\s*(c2|s1)\b"),
    amp("Vox AC50", "amp_chime", ["Amp_EN50"], ["Vox AC50", "Vox AC15"], r"ac[- ]?(50|15)", FULL,
        note="EN-50 (UK-style AC50): AC50 if any, else an AC15"),
    amp("Marshall JMP 2204", "amp_crunch", ["Amp_GB50"], ["Marshall JMP", "Marshall 2204"], r"jmp",
        [4.0, 6.5, 8.5], avoid=r"jmp[- ]?1\b", note="GBC-50 mid-gain British crunch: a JMP master volume"),
    amp("Laney AOR", "amp_lead", ["Amp_GB100"], ["Laney AOR", "Laney"], r"laney", HIGH),
    amp("Soldano SLO-100", "amp_lead", ["Amp_HG100"], ["Soldano SLO", "Soldano"], r"soldano|slo", HIGH),
    amp("Diezel VH4", "amp_modern", ["Amp_HG180"], ["Diezel VH4", "Diezel"], r"diezel", HIGH,
        note="HG-180 'boutique high-gain': a Diezel (closest guess)"),
    amp("Peavey 5150", "amp_modern", ["Amp_HG500"], ["Peavey 5150", "EVH 5150"], r"5150|6505", HIGH),
    amp("Fender 5E3 Tweed Deluxe", "amp_tweed", ["Amp_TW22"], ["Fender Tweed Deluxe", "5E3"], r"5e3|tweed deluxe|tweed",
        CLEAN, avoid=r"blues deluxe"),
    amp("Fender Tweed Bassman", "amp_tweed", ["Amp_TW40"], ["Fender Bassman 5F6", "Fender Tweed Bassman",
                                                          "Fender Bassman"], r"bassman", FULL,
        avoid=r"bassman ?(100|135|50|10)\b|silverface"),
    amp("Gibson GA vintage", "amp_tweed", ["Amp_GibsonGA8", "Amp_GibsonGA79", "Amp_GibsonGA88"],
        ["Gibson GA", "Gibson amp"], r"gibson|\bga[- ]?\d", CLEAN),
    amp("Epiphone vintage", "amp_tweed", ["Amp_EpiphoneElectarCentury", "Amp_EpiphoneElectarM",
                                         "Amp_EpiphoneElectricHawaiian", "Amp_EpiphoneZephyr"],
        ["Epiphone", "Valco", "Supro"], r"epiphone|valco|supro|zephyr|electar", CLEAN,
        note="the 1930s-50s Epiphones: an Epiphone if any, else a Supro/Valco of the time"),
    amp("Marshall Super Lead Plexi", "amp_crunch", ["Amp_MarshallPlexi"], ["Marshall Plexi", "Marshall 1959",
                                                                          "Marshall Super Lead"],
        r"plexi|1959|super lead|1987", [4.0, 6.5, 8.5]),
    amp("Marshall JTM45", "amp_crunch", ["Amp_MarshallJTM45", "Amp_Marshall1962Bluesbreaker", "Amp_GB38"],
        ["Marshall JTM45"], r"jtm ?45", [3.0, 5.5, 8.0],
        note="also the 1962 Bluesbreaker (a JTM45 combo) and GBC-38 low-gain British: no captures of either"),
    amp("Marshall JCM800", "amp_lead", ["Amp_MarshallJCM800"], ["Marshall JCM800", "JCM 800"], r"jcm ?800",
        [3.0, 6.0, 9.0]),
    amp("Marshall DSL", "amp_lead", ["Amp_MarshallDSL15H", "Amp_MarshallDSL100H"], ["Marshall DSL"], r"dsl",
        [2.0, 6.0, 8.5]),
    amp("Marshall JVM410", "amp_lead", ["Amp_MarshallJVM410H"], ["Marshall JVM"], r"jvm", [2.0, 6.0, 9.0]),
    amp("Orange AD30/AD50", "amp_crunch", ["Amp_OrangeAD50"], ["Orange AD30", "Orange AD50"], r"\bad ?(30|50|140)",
        [3.0, 6.0, 8.0]),
    amp("Orange OR50/OR100", "amp_crunch", ["Amp_OrangeOR50H", "Amp_OrangeOR100"],
        ["Orange OR50", "Orange OR100", "Orange OR120", "Orange OR15"], r"\bor ?(15|50|80|100|120)\b",
        [3.0, 6.0, 8.0]),
    amp("Orange Tiny Terror", "amp_crunch", ["Amp_OrangeTinyTerror"], ["Orange Tiny Terror", "Orange Terror"],
        r"terror", [3.0, 6.0, 8.0], avoid=r"jim root|dark"),
    amp("Orange Rockerverb", "amp_lead", ["Amp_OrangeRockerverb"], ["Orange Rockerverb"], r"rockerverb", HIGH),
    amp("Orange Jim Root Terror", "amp_modern", ["Amp_OrangeJimmyBean"], ["Orange Jim Root", "Orange Dark Terror",
                                                                        "Orange CR120"],
        r"jim root|#4|dark terror|cr ?120|crush", HIGH,
        note="Jim Root's amp: his #4 Terror / a Dark Terror / a Crush Pro (solid state)"),
    # ---- bass amps (P37.2: skins on our new bass voices; rs keys from BASS-GEAR.md section 2)
    amp("Ampeg SVT", "amp_bass", ["Bass_Amp_HT300B"], ["Ampeg SVT"], r"svt", [2.0, 5.0], bass=True,
        avoid=r"sansamp|pedal|scr[- ]?di|darkglass"),
    amp("Ampeg B-15 Portaflex", "amp_b15", ["Bass_Amp_LT25B"], ["Ampeg B-15", "Ampeg B15", "Ampeg Portaflex"],
        r"b[- ]?15|portaflex|b[- ]?18", [2.0, 5.0], bass=True),
    amp("Gallien-Krueger 800RB", "amp_bass_ss", ["Bass_Amp_CH300B", "Bass_Amp_CH600B"], ["Gallien Krueger", "GK 800RB"], r"gallien|krueger|\bgk\b|800 ?rb",
        [2.0, 5.0], bass=True),
    amp("Eden WT800", "amp_bass_ss", ["Bass_Amp_EdenE300", "Bass_Amp_EdenWT550", "Bass_Amp_EdenWT800"],
        ["Eden WT", "Eden bass"], r"eden", [2.0, 5.0], bass=True),
    amp("Hartke", "amp_bass_ss", ["Bass_Amp_CS75B"], ["Hartke"], r"hartke", [2.0, 5.0], bass=True, avoid=r"yamaha"),
    amp("Fender Bassman (bass)", "amp_bass", [], ["Fender Bassman bass", "Fender Bassman"], r"bassman",
        [2.0, 5.0], bass=True, note="a Bassman captured with a bass"),
    amp("Orange bass amp", "amp_bass_tube", ["Bass_Amp_HT400B"], ["Orange AD200", "Orange Terror Bass", "Orange bass"],
        r"ad ?200|terror bass|orange.*bass|bass.*orange|ob1", [2.0, 6.0], bass=True),
    amp("Markbass", "amp_bass_ss", ["Bass_Amp_CS75B", "Bass_Amp_BT880B"], ["Markbass"], r"markbass|mark bass", [2.0, 5.0], bass=True),
    # ---- pedals
    pedal("Ibanez TS808", "green", ["Pedal_CustomDrive", "Pedal_SuperDrive"], ["Ibanez TS808", "Tube Screamer"],
          r"ts[- ]?808|ts[- ]?9|tube ?screamer|ts[- ]?10", [2.0, 5.0, 8.0], avoid=r"mini|bass"),
    pedal("Boss SD-1", "green", ["Pedal_SuperDrive"], ["Boss SD-1"], r"sd[- ]?1\b", [3.0, 7.0]),
    pedal("Klon Centaur", "gold", ["Pedal_LineDrive"], ["Klon Centaur", "Klon"], r"klon|centaur", [2.0, 5.0, 8.0]),
    pedal("ProCo RAT", "rodent", ["Pedal_VintageDistortion"], ["ProCo RAT", "Pro Co Rat"], r"\brat\b",
          [3.0, 6.0, 9.0]),
    pedal("Electro-Harmonix Big Muff Pi", "big_pi", ["Pedal_BuzzToo", "Pedal_FuzzWasHe", "Bass_Pedal_BassFuzz"],
          ["Big Muff"], r"big ?muff|muff", [3.0, 6.0, 9.0], avoid=r"bass"),
    pedal("Fuzz Face (germanium)", "round_fuzz", ["Pedal_CaptFuzzle"], ["Fuzz Face germanium", "Fuzz Face", "Dunlop Fuzz Face"],
          r"fuzz ?face|ff\b|jdf2", [4.0, 7.0, 10.0], avoid=r"texas|screamer|\bts\b"),
    pedal("Germanium overdrive", "germanium", ["Pedal_GermaniumDrive"], ["germanium overdrive",
                                                                        "Analogman King of Tone", "Bluesbreaker pedal"],
          r"germanium|king of tone|kot|bluesbreaker|prince of tone", [2.0, 5.0, 8.0], avoid=r"fuzz"),
    pedal("Dallas Rangemaster", "treble_boost", ["Pedal_RangeBooster"], ["Rangemaster", "treble booster"],
          r"rangemaster|range master|treble boost", [5.0, 10.0]),
    pedal("Boss DS-1", "orange", ["Pedal_Distortion"], ["Boss DS-1"], r"ds[- ]?1\b", [3.0, 7.0]),
    pedal("Marshall Guv'nor", "orange", ["Pedal_MarshallGuvnorPlus"], ["Marshall Guvnor"], r"guv", [3.0, 7.0]),
    pedal("Boss MT-2 Metal Zone", "metal", ["Pedal_MetalDistortion", "Pedal_ShredZone"], ["Boss MT-2", "Metal Zone"],
          r"mt[- ]?2|metal ?zone", [4.0, 8.0]),
    pedal("Boss HM-2", "metal", ["Pedal_MetalDistortion"], ["Boss HM-2", "Heavy Metal HM-2"], r"hm[- ]?2",
          [5.0, 9.0]),
    pedal("Boss BD-2 Blues Driver", "blues", [], ["Boss Blues Driver", "BD-2"], r"blues ?driver|bd[- ]?2",
          [3.0, 6.0]),
    pedal("Fulltone OCD", "orange", [], ["Fulltone OCD", "OCD"], r"\bocd\b", [3.0, 7.0]),
    pedal("Tone Bender MkII", "bender", ["Pedal_BuzzOne"], ["Tone Bender"], r"tone ?bender", [5.0, 9.0]),
    pedal("SansAmp Bass Driver DI", "bass_drive", ["DI_Amp_BassDriver", "Bass_Pedal_BassOverdrive",
                                                   "Bass_Pedal_BassDistortion"],
          ["SansAmp Bass Driver", "Bass Driver DI"], r"sansamp|bass ?driver|bddi", [2.0, 5.0, 8.0], bass=True),
    pedal("Darkglass B7K", "bass_drive", ["Bass_Pedal_BassOverdrive", "Bass_Pedal_BassDistortion"],
          ["Darkglass B7K", "Darkglass"], r"darkglass|b7k|microtubes|alpha", [3.0, 7.0], bass=True),
    # ---- cab IRs
    ir("Marshall 1960A 4x12 (G12M greenback)", "cab_4x12",
       ["GB412CMKI", "GB412CMKII", "GB412CMKIII", "Marshall1960a", "Marshall1960AX", "Marshall1960TV", "EN4120C"],
       ["Celestion Greenback 4x12", "Marshall 1960 greenback", "Marshall 1960A"], r"greenback|g12m|g12h", n=3,
       avoid=r"mesa|orange|recto|2x12|1x12"),
    ir("Marshall 1960 4x12 (G12T-75)", "cab_4x12", ["GB412CMKIII", "Marshall1960a"],
       ["Marshall 1960 G12T-75", "G12T75"], r"g12t|t-?75", n=1),
    ir("Mesa Rectifier 4x12 (V30)", "cab_4x12", ["CA412C", "HG212C"], ["Mesa 4x12", "Mesa Rectifier 4x12",
                                                                      "Mesa Oversized"], r"mesa|recto|v30", n=2),
    ir("Orange PPC412", "cab_4x12", ["OrangePPC412", "OrangeJimmyBean"], ["Orange PPC412", "Orange 4x12"],
       r"orange.*(4x12|412)|ppc ?412", n=2),
    ir("Orange PPC212", "cab_2x12", ["OrangePPC212OB"], ["Orange PPC212", "Orange 2x12"],
       r"orange.*(2x12|212)|ppc ?212", n=1),
    ir("Greenback 2x12 (1936-style)", "cab_4x12", ["Marshall1936"], ["Marshall 1936"], r"1936|2x12", n=1),
    ir("Vox AC30 2x12 (Alnico Blue)", "cab_2x12", ["EN212C"], ["Vox AC30 cab", "Alnico Blue 2x12", "AC30 2x12"],
       r"ac ?30|vox", n=2, file=r"ac ?30|vox|blue"),
    ir("Roland JC-120 2x12", "cab_2x12", ["CS212C"], ["JC-120 cab", "Jazz Chorus cab", "Roland JC"],
       r"jc|jazz chorus|roland", n=1),
    ir("Fender Twin 2x12", "cab_2x12", ["HG2120C"], ["Fender Twin cab", "Twin Reverb 2x12", "Fender 2x12"],
       r"twin|2x12|212", n=2),
    ir("Fender Bassman 4x10 (open back)", "cab_2x12", ["TW410C", "BT410C"], ["Fender Bassman 4x10", "4x10 P10R", "Super Reverb 4x10"],
       r"4x10|410|p10", n=2),
    ir("Fender Deluxe 1x12", "cab_1x12", ["AT0112C", "AT1121C", "TW112C", "CA112C", "BT1120C", "BT1121C",
                                          "CS1120C"],
       ["Fender Deluxe Reverb cab", "Fender 1x12", "Deluxe Reverb 1x12"], r"deluxe|1x12|112", n=2),
    ir("Fender Princeton 1x10", "cab_1x10", ["TW110C"], ["Fender Princeton cab", "Princeton 1x10", "1x10 cab"],
       r"princeton|1x10|110", n=2),
    ir("Mesa 2x15 / vintage 2x15", "cab_2x12", ["CA215C", "CS1515C", "HG215C"], ["2x15 cab", "Fender Dual Showman"],
       r"2x15|215|showman", n=1),
    ir("Ampeg SVT-810E", "cab_8x10", ["AT810BC", "OrangeOBC810", "EdenD610XST"], ["Ampeg 8x10", "SVT 810"],
       r"8x10|810", n=2, bass=True, file=r"8x10|810"),
    ir("Bass 4x10", "cab_4x10", ["BT410BC", "CH410BC", "CS410BC", "EdenD410XST"],
       ["Eden 4x10", "Hartke 4x10", "Ampeg 4x10 bass", "bass 4x10"], r"4x10|410", n=2, bass=True,
       file=r"4x10|410"),
    ir("Bass 2x10 / 2x12", "cab_2x10", ["CH210BC", "BT212BC", "EdenD212XLT", "CH310BC"],
       ["bass 2x10", "Hartke 2x10", "Markbass 2x10", "Hartke 2x12 bass"], r"2x10|210|2x12|212", n=1, bass=True,
       avoid=r"bassman"),
    ir("Ampeg B-15 / bass 1x15", "cab_1x15", ["AT1150BC", "BT115BC", "CA1510BC", "CS15BC", "CS112BC",
                                              "EdenD115XLT", "OrangeOBC115", "GB415BC", "TW215BC"],
       ["Ampeg B-15 cab", "bass 1x15", "Ampeg 1x15", "Eden 1x15"], r"1x15|115|b-?15|portaflex", n=2, bass=True),
]

# Second choices: units RS has no key for, but that its tones (and the songs) lean on, by skin. They widen the
# picker's choice for the most used skins.
TARGETS += [
    # lead / modern (amp_lead 2003 tones, amp_modern 935)
    amp("Marshall JCM900", "amp_lead", [], ["Marshall JCM900", "JCM 900"], r"jcm ?900", HIGH),
    amp("Marshall Silver Jubilee", "amp_lead", [], ["Marshall Silver Jubilee", "Marshall 2555"],
        r"jubilee|2555|2525", HIGH),
    amp("Marshall JCM2000 DSL/TSL", "amp_lead", [], ["Marshall JCM2000", "Marshall TSL"], r"jcm ?2000|tsl", HIGH),
    amp("Friedman Dirty Shirley / BE", "amp_lead", [], ["Friedman Dirty Shirley", "Friedman BE"],
        r"dirty shirley|\bbe[- ]?(100|50|od)|brown eye", HIGH),
    amp("Mesa/Boogie Mark IV", "amp_lead", [], ["Mesa Boogie Mark IV"], r"mark ?(iv|4)\b", HIGH),
    amp("EVH 5150 III", "amp_modern", [], ["EVH 5150 III", "EVH 5150"], r"5150 ?iii|5150iii|evh", HIGH),
    amp("ENGL Powerball / Fireball", "amp_modern", [], ["ENGL Powerball", "ENGL Fireball", "ENGL"], r"engl", HIGH),
    amp("Bogner Uberschall", "amp_modern", [], ["Bogner Uberschall"], r"uberschall|überschall", [6.0, 9.0]),
    amp("Mesa/Boogie Triple Rectifier", "amp_modern", [], ["Mesa Triple Rectifier", "Mesa Rectifier"],
        r"triple|recto ?verb|rectifier", [6.0, 9.0], avoid=r"badlander|dual"),
    # crunch / chime / clean / tweed
    amp("Hiwatt DR103", "amp_crunch", [], ["Hiwatt DR103", "Hiwatt"], r"hiwatt", [3.0, 6.0, 8.0]),
    amp("Marshall 1987X / Plexi 50W", "amp_crunch", [], ["Marshall 1987", "Marshall Plexi 50"], r"1987|plexi",
        [4.0, 6.5, 8.5]),
    amp("Vox AC30 (second)", "amp_chime", ["Amp_EN30"], ["Vox AC30 Top Boost", "AC30 Top Boost"], r"ac[- ]?30",
        FULL),
    amp("Bad Cat / Dr. Z", "amp_chime", [], ["Dr Z Maz", "Bad Cat", "Dr Z"], r"dr\.? ?z|bad ?cat|maz ?(18|38)",
        FULL),
    amp("Fender Super Reverb", "amp_clean", [], ["Fender Super Reverb"], r"super reverb", CLEAN),
    amp("Fender Bassman silverface (guitar)", "amp_clean", [], ["Fender Silverface Bassman", "Fender Bassman 50"],
        r"bassman", CLEAN, avoid=r"5f6|tweed"),
    amp("Fender Blues Junior", "amp_clean", [], ["Fender Blues Junior"], r"blues jr|blues junior", CLEAN),
    amp("Dumble Overdrive Special", "amp_clean", [], ["Dumble ODS", "Dumble"], r"dumble|ods\b", [2.0, 6.0]),
    amp("Fender Champ 5F1", "amp_tweed", [], ["Fender Champ 5F1", "Fender Champ"], r"champ", CLEAN),
    amp("Fender Tweed Twin / Pro", "amp_tweed", [], ["Fender Tweed Twin", "Fender Tweed Pro", "Fender 5E8"],
        r"tweed|5e8|5e5|high power twin", CLEAN, avoid=r"5e3|deluxe|bassman"),
    # bass (amp_bass 1639)
    amp("Ampeg SVT (second)", "amp_bass", ["Bass_Amp_HT300B"], ["Ampeg SVT VR", "Ampeg SVT-4", "Ampeg SVT Classic"],
        r"svt", [3.0, 6.0, 8.0], bass=True, avoid=r"sansamp|pedal|scr[- ]?di|darkglass"),
    amp("Aguilar Tone Hammer / DB750", "amp_bass_tube", ["Bass_Amp_BT975B"], ["Aguilar"], r"aguilar", [2.0, 5.0], bass=True,
        avoid=r"darkglass"),
    amp("Trace Elliot", "amp_bass", [], ["Trace Elliot"], r"trace ?elliot", [2.0, 5.0], bass=True),
    pedal("Ampeg SCR-DI", "bass_drive", [], ["Ampeg SCR-DI"], r"scr[- ]?di", [2.0, 7.0], bass=True),
    pedal("Tech 21 VT Bass", "bass_drive", [], ["Tech21 VT Bass", "VT Bass DI"], r"vt ?bass", [3.0, 7.0], bass=True),
    # pedals
    pedal("Klon Centaur (original)", "gold", ["Pedal_LineDrive"], ["Klon Centaur", "Klon KTR"],
          r"klon|centaur|ktr", [2.0, 5.0, 8.0], avoid=r"behringer|klone|archer"),
    pedal("Analogman King of Tone", "germanium", [], ["King of Tone", "Prince of Tone"],
          r"king of tone|prince of tone|\bkot\b|\bpot\b", [2.0, 5.0, 8.0]),
    pedal("Marshall Bluesbreaker pedal", "germanium", [], ["Marshall Bluesbreaker pedal",
                                                                               "Bluesbreaker pedal"],
          r"bluesbreaker", [3.0, 7.0]),
    pedal("Fuzz Face (silicon)", "round_fuzz", ["Pedal_CaptFuzzle"], ["Fuzz Face silicon", "Dunlop Fuzz Face mini",
                                                                       "Fuzz Face BC108"],
          r"fuzz ?face|bc108|silicon", [5.0, 9.0], avoid=r"texas|screamer|\bts\b|eric johnson"),
    pedal("Ibanez TS9 (second)", "green", ["Pedal_CustomDrive", "Pedal_SuperDrive"], ["Ibanez TS9", "TS808"],
          r"ts[- ]?9|ts[- ]?808", [3.0, 6.0], avoid=r"mini|bass"),
    pedal("Boss DS-2 / Turbo Distortion", "orange", [], ["Boss DS-2"], r"ds[- ]?2\b", [4.0, 8.0]),
    pedal("Boss SD-1 (second)", "green", ["Pedal_SuperDrive"], ["Boss SD-1 Super Overdrive"], r"sd[- ]?1\b",
          [2.0, 5.0, 9.0]),
    pedal("Electro-Harmonix Big Muff Pi (Russian)", "big_pi", ["Pedal_BuzzToo", "Pedal_FuzzWasHe"],
          ["Sovtek Big Muff", "Russian Big Muff"], r"muff", [4.0, 8.0], avoid=r"op-?amp|\bic\b|bass"),
    pedal("Boss AC-3 Acoustic Simulator", "acoustic", ["Pedal_AcousticEmulator"], ["Boss AC-3", "acoustic simulator"],
          r"ac[- ]?3\b|acoustic sim", [2.0]),
    # cab IRs for the most used bodies (cab_4x12 3781 tones, cab_2x12 1419, cab_8x10 1035, cab_1x12 934)
    ir("Marshall 1960AV 4x12 (V30)", "cab_4x12", [], ["Marshall 1960AV", "Marshall 1960BV"],
       r"1960 ?[ab]v", n=2),
    ir("EVH 5150 III 4x12", "cab_4x12", [], ["EVH 5150 4x12", "EVH 412"], r"evh|5150", n=1),
    ir("Bogner / Friedman closed cab", "cab_4x12", [], ["Friedman 4x12", "Bogner 4x12"], r"friedman|bogner", n=2,
       file=r"4x12|412"),
    ir("Mesa 4x12 (second)", "cab_4x12", ["CA412C"], ["Mesa Boogie 4x12 SM57", "Mesa Standard 4x12"],
       r"mesa", n=2),
    ir("Matchless / Bogner 2x12", "cab_2x12", [], ["Matchless 2x12", "Bogner 2x12"],
       r"matchless|bogner", n=1),
    ir("Mesa 2x12", "cab_2x12", [], ["Mesa 2x12"], r"mesa.*(2x12|212)", n=1),
    ir("Vox AC15 1x12", "cab_1x12", [], ["Vox AC15 cab", "AC15 1x12"], r"ac ?15", n=1),
    ir("Mesa 1x12", "cab_1x12", ["CA112C"], ["Mesa 1x12"], r"mesa.*(1x12|112)", n=1),
    ir("Fender Champ / tweed 1x8-1x10", "cab_1x10", ["TW110C"], ["Fender Champ cab", "tweed 1x10"],
       r"champ|1x10|1x8|110", n=1),
    ir("Ampeg SVT-810E (second)", "cab_8x10", ["AT810BC"], ["SVT 810E", "Ampeg SVT 8x10"], r"8x10|810", n=2, bass=True,
       file=r"8x10|810"),
    ir("Bass 1x15 (second)", "cab_1x15", ["EdenD115XLT", "OrangeOBC115"], ["Eden 1x15", "Fender Bassman 1x15",
                                                                          "Ampeg B15 cab"],
       r"1x15|115|b-?15", n=2, bass=True),
]

BASS_GEAR = r"svt|b-?15|eden|hartke|gallien|krueger|sansamp|darkglass|markbass|portaflex|ad ?200|terror bass|8x10|810"

# the mics/positions to prefer in an IR pack, best first
MIC_PREF = [r"sm ?57.*(cap ?edge|edge|cone|cap)", r"sm ?57", r"57", r"md ?421|421", r"e ?906|906", r"r ?121|121",
            r"close|dyn", r""]

# --------------------------------------------------------------------------------------------------------------
# API


_last = [0.0]
_count = [0]


def key():
    k = os.environ.get("T3K_SECRET")
    if not k:
        sys.exit("set T3K_SECRET to the TONE3000 API secret key")
    return k


def _get(url, raw=False):
    for attempt in range(8):
        wait = 0.75 - (time.time() - _last[0])
        if wait > 0:
            time.sleep(wait)
        _last[0] = time.time()
        _count[0] += 1
        req = urllib.request.Request(url, headers={"Authorization": "Bearer " + key(), "User-Agent": "quickRS-harvest"})
        try:
            with urllib.request.urlopen(req, timeout=60) as r:
                body = r.read()
            return body if raw else json.loads(body)
        except urllib.error.HTTPError as e:
            if e.code in (429, 500, 502, 503, 504):
                back = float(e.headers.get("Retry-After") or 0) or min(60, 5 * 2 ** attempt)
                print(f"  {e.code}, waiting {back:.0f}s", flush=True)
                time.sleep(back)
                continue
            raise
        except (urllib.error.URLError, TimeoutError) as e:
            print(f"  network: {e}, retrying", flush=True)
            time.sleep(5 * (attempt + 1))
    raise RuntimeError("gave up on " + url.split("?")[0])


def api(path, **params):
    q = urllib.parse.urlencode(params)
    name = re.sub(r"[^A-Za-z0-9]+", "_", path + "_" + q)[:180] + ".json"
    f = CACHE / name
    if f.exists():
        return json.loads(f.read_text(encoding="utf-8"))
    d = _get(f"{API}{path}?{q}" if q else f"{API}{path}")
    CACHE.mkdir(parents=True, exist_ok=True)
    f.write_text(json.dumps(d), encoding="utf-8")
    return d


def search(query, fmt, gears):
    return api("/tones/search", query=query, page_size=25, format=fmt, gears=gears,
               sort="downloads-all-time").get("data", [])


def models(tone_id):
    return api("/models", tone_id=tone_id, page_size=100).get("data", [])


# --------------------------------------------------------------------------------------------------------------
# Choosing


def text_of(t):
    return " ".join([t.get("title") or ""] + [m.get("name", "") for m in t.get("makes") or []]).lower()


def gears_for(t):
    if t["kind"] == "ir":
        return "cab"
    if t["kind"] == "pedal":
        return "pedal"
    return "amp"


def candidates(t):
    """Matching tones, best first: the unit in the title before the unit only in the makes, then by downloads."""
    fmt = "ir" if t["kind"] == "ir" else "nam"
    seen, out = set(), []
    for q in t["q"]:
        for tone in search(q, fmt, gears_for(t)):
            if tone["id"] in seen:
                continue
            seen.add(tone["id"])
            title = (tone.get("title") or "").lower()
            txt = text_of(tone)
            if not re.search(t["must"], txt):
                continue
            if t.get("avoid") and re.search(t["avoid"], txt):
                continue
            desc = (tone.get("description") or "")[:400].lower()
            is_bass = bool(re.search(r"\bbass\b", txt))
            if t["bass"]:
                if not (is_bass or re.search(r"\bbass\b", desc) or re.search(BASS_GEAR, txt)):
                    continue
            elif is_bass:
                continue
            tone["_tier"] = 0 if re.search(t["must"], title) else 1
            out.append(tone)
    out.sort(key=lambda x: (x["_tier"], -(x.get("downloads_count") or 0)))
    return out


# a knob's o'clock position as 0-10
CLOCK = {7: 0, 8: 1, 9: 2.5, 10: 3.5, 11: 4.5, 12: 5, 1: 6, 2: 7, 3: 8, 4: 9, 5: 10}
NUM = r"[\s:=@_-]*(max|\d+(?:[.,]\d+)?)(?!\s*(?:ms|hz|khz|db|w\b|watt|v\b|volt|%?\s*dbu|th\b))"
# knob names in the order they say "gain": a gain knob beats a drive knob beats a volume knob
# a one-letter knob ("G7", "D-5") only with a one- or two-digit setting
ONE = r"(?=[\s:=_-]*\d{1,2}(?:[.,]\d)?(?![\d.]))"
GAIN_KEYS = [r"(?:gain|\bg" + ONE + r"|\bgn)", r"(?:drive|\bdrv|\bd" + ONE + r"|\bod)",
             r"(?:dist(?:ortion)?|fuzz|sustain|\bs" + ONE + r"|smash|dirt)", r"(?:vol(?:ume)?|\bvl)", r"\bv" + ONE]


def _num(v):
    if v == "max":
        return 10.0
    x = float(v.replace(",", "."))
    if x >= 100 and len(v) in (3, 4) and v.isdigit() and int(v[:-2]) in CLOCK:
        return float(CLOCK[int(v[:-2])])  # "Drive1200", "DIST-0900": o'clock
    if x > 10:
        return x / 10 if x <= 100 else None
    return x


def gain_of(name):
    """Estimated gain 0-10 from a model's name, or None."""
    n = name.lower().replace("_", " ")
    n = re.sub(r"\bv\d+ pulled|in v\d+\b|\bv\d(?:\.\d)+|\bv\d+\s*$", " ", n)  # tube sockets, versions
    n = re.sub(r"\bv(\d)-(\d+)", r"vol \2", n, count=1)  # a 5E3's "V1-10 V2-0": the first volume
    knobs = len(re.findall(r"\b[a-z]{1,2}\d{1,2}\b", n))
    g = None
    for i, k in enumerate(GAIN_KEYS):
        if i == len(GAIN_KEYS) - 1 and knobs < 2:
            break  # "V2" alone is a version, "V6 T5 B4" a volume knob
        m = re.search(k + NUM, n)
        if m:
            raw = m.group(1)
            clocks = len(re.findall(r"\b[a-z]{1,3}(1[1-7])\b", n))
            if raw.isdigit() and 7 <= int(raw) <= 17 and clocks >= (1 if int(raw) >= 11 else 2) and \
                    re.search(r"\b[a-z]{1,3}12\b", n):
                h = int(raw) if int(raw) <= 12 else int(raw) - 12  # "V12 T12 G15": knobs as o'clock
                g = float(CLOCK[h]) if h in CLOCK else None
            else:
                g = _num(raw)
            if g is not None:
                break
    if g is None:
        m = re.search(r"(\d{1,2})\s*(?:o'?clock|oclock)", n)
        if m and int(m.group(1)) in CLOCK:
            g = float(CLOCK[int(m.group(1))])
    if g is None:
        if re.search(r"\bmax(ed)?\b|\bfull ?(gain|crank)|dimed|cranked|full ?on", n):
            g = 9.5
        elif re.search(r"high ?gain|hi[- ]?gain|\bhg\b|lead|solo|insane|brutal|metal|chug|ultra|\bod ?[12]\b|"
                       r"\bred\b|modern|djent|thrash|doom", n):
            g = 8.5
        elif re.search(r"\bod\b|overdrive|drive|dirt|rhythm|\bdist|\bhot\b|\bmg\b", n):
            g = 6.5
        elif re.search(r"crunch|edge|break ?up|breakup|grit|push|\bnoon", n):
            g = 5.0
        elif re.search(r"clean|jazz|sparkl|glass|low ?gain|\bcln\b|\blg\b", n):
            g = 2.0
        elif re.search(r"\bmin\b|\boff\b|unity|bypass", n):
            g = 1.0
    if g is not None and re.search(r"clean|\bcln\b", n) and not re.search(r"boost", n):
        g = min(g, 4.0)  # the clean channel's gain knob
    return None if g is None else round(min(10.0, max(0.0, g)), 1)


# "custom" WaveNets (the "Hyper Accuracy" packs) cost ~90% of a core here (nam bench), a standard one ~19%: never
SIZE_RANK = {"standard": 0, None: 0, "lite": 2, "feather": 3, "nano": 4, "custom": 9}


def model_pool(t, cands, ntones=3):
    """(rank, gain, tone, model) for the models of the best tones, one per distinct capture name."""
    pool = []
    rank = 0
    for tone in cands[:ntones + 3]:  # the best ntones tones that have usable models (not all custom/cab)
        if rank >= ntones:
            break
        seen = {}
        for m in models(tone["id"]):
            if m["id"] in TAKEN:
                continue
            nm = (m.get("name") or "").strip()
            low = nm.lower()
            if t.get("avoid") and re.search(t["avoid"], low):
                continue
            if t["kind"] == "nam" and re.search(r"\b(cab|ir|full ?rig|di ?\+ ?cab|w ?cab)\b", low) and not \
                    re.search(r"no ?cab|w/?o cab|without cab|\bdi\b|no ?ir|amp only|head only", low):
                continue
            if SIZE_RANK.get(m.get("size"), 5) > 2:
                continue
            base = re.sub(r"[\s\-_(\[]*(a1|a2|v1|v2|standard|std|lite|custom)[)\]]*\s*$", "", low)
            cur = seen.get(base)
            if cur is None or SIZE_RANK.get(m.get("size"), 5) < SIZE_RANK.get(cur.get("size"), 5):
                seen[base] = m
        for m in seen.values():
            pool.append((rank + 0.5 * SIZE_RANK.get(m.get("size"), 5), gain_of(m.get("name") or ""), tone, m))
        if seen:
            rank += 1
    return pool


def pick_models(t, cands):
    pool = model_pool(t, cands)
    if not pool:
        return []
    known = [p for p in pool if p[1] is not None]
    picks = []
    if len(known) >= 2:
        for want in t["gains"]:
            free = [p for p in known if p[3]["id"] not in {q[3]["id"] for q in picks}
                    and all(abs(p[1] - q[1]) >= 1.2 for q in picks)]
            if not free:
                break
            best = min(free, key=lambda p: abs(p[1] - want) + 0.8 * p[0])
            if abs(best[1] - want) <= 2.5 or not picks:
                picks.append(best)
    if len(picks) < min(2, len(t["gains"])):
        # few gains in the names: fill from the best tone, spread over its list, gain from the target's points
        top = [p for p in pool if p[0] == min(q[0] for q in pool) and p not in picks]
        for i, want in enumerate(t["gains"][len(picks):]):
            if not top:
                break
            p = top[round(i * (len(top) - 1) / max(1, len(t["gains"]) - 1))] if len(top) > 1 else top[0]
            top.remove(p)
            picks.append((p[0], p[1] if p[1] is not None else want, p[2], p[3]))
    return [(g, tone, m) for _, g, tone, m in picks]


def pick_irs(t, cands):
    """The n best-miked files over the top tones (an SM57 on the cone edge first), one per tone first."""
    scored = []
    for rank, tone in enumerate(cands[:4]):
        names = set()
        for m in models(tone["id"]):
            if m["id"] in TAKEN:
                continue
            nm = (m.get("name") or "").strip()
            low = nm.lower()
            if not nm or low in names:
                continue
            names.add(low)
            bad = bool(re.search(r"room|\bfar\b|ambien|stereo|\bdi\b|\bdry\b|\boff ?axis|back|rear|"
                                 r"[2-9] ?mics?|\+", low))
            pref = next(i for i, p in enumerate(MIC_PREF) if re.search(p, low))
            if t.get("file") and not re.search(t["file"], low):
                pref += 6
            scored.append((pref + (4 if bad else 0) + 1.5 * rank, tone, m))
    out = []
    while scored and len(out) < t["n"]:
        used = {o[1]["id"] for o in out}
        best = min(scored, key=lambda x: x[0] + (1.0 if x[1]["id"] in used else 0.0))
        scored.remove(best)
        out.append((None, best[1], best[2]))
    return out


# models already in the library or picked by an earlier target: a second target for a unit brings new ones
TAKEN = set()


def plan_target(t):
    cands = candidates(t)
    picks = pick_irs(t, cands) if t["kind"] == "ir" else pick_models(t, cands)
    TAKEN.update(m["id"] for _, _, m in picks)
    return cands, picks


# --------------------------------------------------------------------------------------------------------------
# Writing


def safe(s, n=60):
    s = s.replace("&", "and").replace("+", " plus ").replace("/", " ").replace("'", "")
    s = re.sub(r"[^A-Za-z0-9 \-]+", " ", s)
    s = re.sub(r"\s+", " ", s).strip(" -")
    return s[:n].strip(" -")


def cli(*args):
    try:
        r = subprocess.run([str(CLI), *map(str, args)], capture_output=True, text=True, timeout=300)
        return r.returncode == 0, (r.stdout + r.stderr)
    except subprocess.TimeoutExpired:
        return False, "timeout"


# words in model names that say nothing about the setting (capturers' tags, rigs, training notes)
NOISE = {"tim", "r", "slammin", "gp", "st", "drps", "pwc", "app", "emil", "rohbe", "canov", "arnold", "hyper",
         "accuracy", "dbu", "18dbu", "standard", "std", "epocs", "epochs", "esr", "azg", "nam", "capture", "captures",
         "s", "c", "o", "di", "bal", "in", "the", "and", "with", "w", "amp", "head", "only", "tf", "enhanced", "xr",
         "pack", "ir", "irs", "jp", "is", "out", "of", "tune", "a1", "a2", "v1", "v2", "mix", "ready", "#"}


def gain_word(g, kind):
    if g is None:
        return ""
    if kind == "pedal":
        return f"drive {g:g}"
    return "clean" if g < 3.5 else "edge" if g < 5.0 else "crunch" if g < 7.5 else "high gain"


def unit_of(t, tone):
    """The real unit a capture is of: the tone's make when it is the target's unit, else the target's name."""
    for mk in tone.get("makes") or []:
        name = re.sub(r"\s+-\s+", " ", mk.get("name") or "").strip()
        if re.search(t["must"], name.lower()) and not (t.get("avoid") and re.search(t["avoid"], name.lower())) \
                and 3 <= len(name) <= 40 and " " in name \
                and not re.match(r"(jensen|celestion|eminence|shure|beyerdynamic|sennheiser|g12)", name.lower()):
            return name
    return re.sub(r"\s*\(second\)", "", t["gear"])


def setting_of(t, tone, m, g):
    """The shown setting: how driven it is, then what is left of the model's name once the unit's words are gone."""
    nm = re.sub(r"\.(nam|wav)$", "", m.get("name") or "", flags=re.I)
    known = {w.lower() for w in re.findall(r"[A-Za-z0-9']+", " ".join(
        [t["gear"], tone.get("title") or ""] + [mk.get("name") or "" for mk in tone.get("makes") or []]))}
    words = [w for w in re.split(r"[\s_|,()\[\]/+\-]+", nm) if w]
    keep = [w for w in words if w.lower().strip("'") not in known and w.lower() not in NOISE
            and not re.fullmatch(r"\d{3,}", w)]
    while keep and re.fullmatch(r"\d{1,2}\.?", keep[0]):
        keep.pop(0)  # a capture's number in its pack
    if all(re.fullmatch(r"[\d.]+", w) for w in keep):
        keep = []
    rest = " ".join(keep)[:30].strip()
    word = gain_word(g, t["kind"]) if t["kind"] != "ir" else ""
    out = f"{word} {rest}".strip() if rest.lower() != word else word
    return out or (nm[:30] if nm else "capture")


def save_json(path, data):
    tmp = path.with_name(path.name + ".tmp")
    tmp.write_text(json.dumps(data, indent=1, ensure_ascii=False), encoding="utf-8")
    os.replace(tmp, path)


# the fields the picker reads, for the items that were there before the harvest (ids 1-10)
OLD_ITEMS = {
    1: dict(skin="amp_clean", gain=3.0), 2: dict(skin="amp_clean", gain=5.0), 3: dict(skin="amp_clean", gain=8.0),
    4: dict(skin="amp_clean", gain=6.0), 5: dict(skin="amp_clean", gain=7.0), 6: dict(skin="amp_tweed", gain=8.0),
    7: dict(skin="cab_2x12"), 8: dict(skin="cab_1x12"), 9: dict(skin="green", gain=6.0),
    10: dict(skin="orange", gain=6.0),  # catalog brand(): orange = Boss DS-1
}


def run(dry=False, only=None):
    db_path = LIB / "captures.json"
    db = json.loads(db_path.read_text(encoding="utf-8"))
    items = db["items"]
    for i in items:
        extra = OLD_ITEMS.get(i["id"])
        if extra:
            for k, v in {**extra, "rs": [], "bass": False}.items():
                i.setdefault(k, v)
    have_models = {i.get("model_id") for i in items if i.get("model_id")}
    have_hash = {hashlib.sha1((LIB / i["file"]).read_bytes()).hexdigest() for i in items if (LIB / i["file"]).exists()}
    for i in items:  # the items from before the harvest have no model id: find theirs, so they aren't fetched again
        if not i.get("model_id") and i.get("tone_id"):
            words = re.findall(r"[a-z0-9]+", i["name"].split("·")[-1].lower())
            for m in models(i["tone_id"]):
                if words and all(w in (m.get("name") or "").lower() for w in words if w not in ("full", "rig")):
                    TAKEN.add(m["id"])
    nid = max([FIRST_ID - 1] + [i["id"] for i in items]) + 1
    stage = WORK / "stage"
    stage.mkdir(parents=True, exist_ok=True)
    report = {"added": [], "empty": [], "failed": [], "short": []}
    test_amp = next((LIB / i["file"] for i in items if i.get("kind") == "nam" and (LIB / i["file"]).exists()), None)
    for t in TARGETS:
        if only and only.lower() not in t["gear"].lower():
            continue
        cands, picks = plan_target(t)
        want = t["n"] if t["kind"] == "ir" else len(t["gains"])
        print(f"== {t['gear']}: {len(picks)}/{want} picks from {len(cands)} tones", flush=True)
        if not picks:
            report["empty"].append(t["gear"])
            continue
        got = 0
        for g, tone, m in picks:
            if m["id"] in have_models:
                got += 1
                continue
            print(f"   {tone['id']} {tone['title'][:40]!r} / {m['name'][:50]!r} gain={g}", flush=True)
            if dry:
                continue
            ext = ".wav" if t["kind"] == "ir" else ".nam"
            dl = WORK / "dl" / f"{m['id']}{ext}"  # downloads kept, so a rerun doesn't fetch them again
            if dl.exists():
                body = dl.read_bytes()
            else:
                url = m.get("model_url") or api(f"/models/{m['id']}").get("model_url")
                try:
                    body = _get(url, raw=True)
                except Exception as e:  # noqa: BLE001
                    report["failed"].append(f"{t['gear']} / {m['name']}: download {e}")
                    continue
                dl.parent.mkdir(parents=True, exist_ok=True)
                dl.write_bytes(body)
            tmp = stage / f"{m['id']}{ext}"
            digest = hashlib.sha1(body).hexdigest()
            if digest in have_hash:  # the same file under another tone
                report["failed"].append(f"{t['gear']} / {m['name']}: same file as one already in the library")
                continue
            tmp.write_bytes(body)
            if ext == ".nam":
                ok, out = cli("nam", "info", tmp)
            elif body[:4] != b"RIFF":
                ok, out = False, "not a RIFF wav"
            elif test_amp:
                ok, out = cli("nam", "render", test_amp, "--ir", tmp, "-o", stage / "render.wav")
            else:
                ok, out = True, ""
            if not ok:
                report["failed"].append(f"{t['gear']} / {m['name']}: {out.strip()[-160:]}")
                tmp.unlink(missing_ok=True)
                continue
            setting = setting_of(t, tone, m, g)
            unit = unit_of(t, tone)
            if t["kind"] == "ir" and not re.search(r"cab|\d ?x ?\d|\b\d{3,4}[a-z]*\b", unit.lower()):
                unit += " cab"
            stem = safe(f"{unit} {setting}", 60 - len(str(nid)) - 1 - len(ext))
            fname = f"{nid} {stem}{ext}"
            dest = LIB / fname
            os.replace(tmp, dest)
            if ext == ".nam" and t["kind"] == "nam" and not t["bass"]:
                test_amp = test_amp or dest
            user = tone.get("user") or {}
            cab_line = next((x for x in out.lower().splitlines() if x.strip().startswith("cab")), "")
            item = {
                "id": nid,
                "kind": t["kind"],
                "name": f"{unit} · {setting}",
                "file": fname,
                "added": time.time(),
                "source": "tone3000",
                "creator": user.get("display_name") or user.get("username") or "",
                "licence": tone.get("license") or "",
                "tone_id": tone["id"],
                "tone_url": tone.get("url") or "",
                "model_id": m["id"],
                "gear": unit,
                "cab": t["kind"] == "nam" and "in the capture" in cab_line,
                "size": m.get("size") or "standard",
                "skin": t["skin"],
                "rs": list(t["rs"]),
                "bass": bool(t["bass"]),
            }
            if t["kind"] != "ir":
                item["gain"] = round(float(g), 1) if g is not None else None
            items.append(item)
            have_hash.add(digest)
            have_models.add(m["id"])
            report["added"].append(item)
            got += 1
            nid += 1
            if len(report["added"]) % 50 == 0:
                db["next"] = nid
                save_json(db_path, db)
        if got < want:
            report["short"].append(f"{t['gear']}: {got}/{want}")
    if not dry:
        db["next"] = max([nid, db.get("next", 0)] + [i["id"] + 1 for i in items])
        save_json(db_path, db)
    WORK.mkdir(parents=True, exist_ok=True)
    (WORK / "report.json").write_text(json.dumps(report, indent=1, ensure_ascii=False), encoding="utf-8")
    print(f"added {len(report['added'])}, empty {report['empty']}, short {report['short']}, "
          f"failed {len(report['failed'])}, requests {_count[0]}")


def explore(only=None):
    for t in TARGETS:
        if only and only.lower() not in t["gear"].lower():
            continue
        cands, picks = plan_target(t)
        print(f"== {t['gear']} [{t['skin']}]  candidates:")
        for c in cands[:4]:
            print(f"     {c['id']:>6} {c.get('downloads_count', 0):>7} t{c['_tier']} {c.get('license')}"
                  f"  {c['title'][:60]!r}")
        for g, tone, m in picks:
            print(f"   -> {tone['id']} {m['id']} {m.get('size')} gain={g} {m['name'][:50]!r}"
                  f"  => {t['gear']} · {setting_of(t, tone, m, g)}")
    print("requests", _count[0])


if __name__ == "__main__":
    a = sys.argv[1:]
    if not a or a[0] not in ("explore", "run"):
        sys.exit(__doc__)
    rest = [x for x in a[1:] if not x.startswith("--")]
    if a[0] == "explore":
        explore(rest[0] if rest else None)
    else:
        run(dry="--dry" in a, only=rest[0] if rest else None)
