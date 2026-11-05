//! The game's real file, folder and device names, which quickRS looks for on the user's disk and in device lists.
//! Everywhere else the game is called RS (D149); these keep the full name, because changing them would stop
//! quickRS finding the game's songs, its audio setup or its guitar cable. New code looks them up here.

/// The game's folder under Steam's `steamapps/common` (holding `dlc`, the program and `Rocksmith.ini`).
pub const GAME_FOLDER: &str = "Rocksmith2014";
/// The game's program, next to its `dlc` folder.
pub const GAME_EXE: &str = "Rocksmith2014.exe";
/// The game's settings file ([Audio]: exclusive mode, latency buffer, ...), next to its program.
pub const GAME_INI: &str = "Rocksmith.ini";
/// The Remastered edition's install folder name (the Python app's default library is its `dlc`).
pub const REMASTERED_FOLDER: &str = "Rocksmith 2014 Edition - Remastered";
/// Part of the Real Tone Cable's device name ("Rocksmith Guitar Adapter"), lowercase for matching.
pub const CABLE_NAME_PART: &str = "rocksmith";

#[cfg(test)]
mod tests {
    use super::*;

    /// These are the names on disk: a rename would lose the user's songs and setup.
    #[test]
    fn the_real_names_stay_the_real_names() {
        assert_eq!(GAME_FOLDER, ["Rock", "smith2014"].concat());
        assert_eq!(GAME_EXE, format!("{GAME_FOLDER}.exe"));
        assert_eq!(GAME_INI, ["Rock", "smith.ini"].concat());
        assert_eq!(
            REMASTERED_FOLDER,
            ["Rock", "smith 2014 Edition - Remastered"].concat()
        );
        assert_eq!(CABLE_NAME_PART, ["rock", "smith"].concat());
    }
}
