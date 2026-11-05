# NAM capture harvest

`harvest.py` builds the owner's personal NAM library from TONE3000: for each real amp, pedal and cab behind quickRS's
RS mapping (`TARGETS`, from `board/rs_gear.rs` and `catalog.rs` `brand()`) it picks well-downloaded
captures spread over gain, checks each with `quickrs-cli nam info` (IRs with `nam render --ir`), and appends them to
`%APPDATA%\quickRS\data\captures\captures.json` with `skin`, `rs`, `gain` and `bass` for the picker (new ids from 100).

Rerun: set `T3K_SECRET` (the TONE3000 API secret key; never commit it), optionally `T3K_WORK` (cache folder), then
`python tools/nam/harvest.py explore` to see the picks, `python tools/nam/harvest.py run` to download. Reruns skip
models already in the library (by `model_id`) and reuse the response cache.
