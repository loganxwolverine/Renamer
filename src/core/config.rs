use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// Clé API TMDB v3 (32 caractères) ou jeton de lecture v4 (commence par « eyJ »)
    pub tmdb_key: String,
    /// Langue des titres TMDB (fr-FR)
    pub language: String,
    /// Dernier dossier ouvert
    pub last_root: Option<PathBuf>,

    pub movie_folder: String,
    pub movie_file: String,
    pub series_folder: String,
    pub season_folder: String,
    pub episode_file: String,
    /// Composition de {tech}
    pub tech: String,
    /// false (défaut) : on renomme sur place, l'arborescence ne bouge pas.
    /// true : on range dans « Film (année)/ » et « Série/Saison XX/ ».
    pub reorganize: bool,
    /// N'ajouter le titre original que s'il est en alphabet latin (évite 千と千尋の神隠し)
    pub original_title_latin_only: bool,
    /// Analyses ffprobe en parallèle (réduire sur un partage SMB lent)
    pub probe_threads: usize,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            tmdb_key: String::new(),
            language: "fr-FR".into(),
            last_root: None,
            movie_folder: "{titre}< ({annee})>< [{titre_original}]>< [tmdbid-{tmdbid}]>".into(),
            movie_file: "{titre}< ({annee})>< [{titre_original}]>< [tmdbid-{tmdbid}]>< - {tech}>".into(),
            series_folder: "{serie}< ({annee})>< [{titre_original}]>< [tmdbid-{tmdbid}]>".into(),
            season_folder: "Saison {saison}".into(),
            episode_file: "{serie} - S{saison}E{episode}< - {titre_episode}>< - {tech}>".into(),
            tech: "{source} {resolution} {codec} {hdr} {audio} {langues}".into(),
            reorganize: false,
            original_title_latin_only: true,
            probe_threads: 4,
        }
    }
}

impl Config {
    pub fn dir() -> PathBuf {
        dirs::config_dir().unwrap_or_else(|| PathBuf::from(".")).join("openhtpc-renamer")
    }
    fn file() -> PathBuf {
        Self::dir().join("config.json")
    }
    pub fn load() -> Self {
        std::fs::read_to_string(Self::file())
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }
    pub fn save(&self) -> anyhow::Result<()> {
        std::fs::create_dir_all(Self::dir())?;
        let f = Self::file();
        std::fs::write(&f, serde_json::to_string_pretty(self)?)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o600));
        }
        Ok(())
    }
}
