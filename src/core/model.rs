use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Forme physique du média sur le disque.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Layout {
    /// Fichier vidéo simple (mkv, mp4, m2ts, avi…)
    File,
    /// Dossier DVD (contient VIDEO_TS)
    DvdFolder,
    /// Dossier Blu-ray / UHD (contient BDMV)
    BlurayFolder,
    /// Image disque .iso
    Iso,
}

impl Layout {
    pub fn label(&self) -> &'static str {
        match self {
            Layout::File => "Fichier",
            Layout::DvdFolder => "Dossier DVD",
            Layout::BlurayFolder => "Dossier Blu-ray",
            Layout::Iso => "Image ISO",
        }
    }
    pub fn is_dir(&self) -> bool {
        matches!(self, Layout::DvdFolder | Layout::BlurayFolder)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MediaKind {
    Movie,
    Episode,
}

/// Ce qu'on a pu deviner à partir du nom de fichier / des dossiers.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Guess {
    pub title: String,
    pub year: Option<u16>,
    pub season: Option<u16>,
    pub episodes: Vec<u16>,
    /// Numéro de disque (DISC2, CD1, Disque 2…)
    pub disc: Option<u8>,
    /// Tags techniques repérés dans le nom (utilisés si ffprobe échoue)
    pub tags: Vec<String>,
}

impl Guess {
    pub fn is_episode(&self) -> bool {
        !self.episodes.is_empty()
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct AudioTrack {
    pub format: String,   // "TrueHD Atmos", "DTS-HD MA", "AC3"…
    pub channels: String, // "7.1", "5.1"…
    pub lang: Option<String>,
    pub score: u32,
}

/// Informations techniques réelles.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TechInfo {
    pub source: Option<String>,     // "UHD Bluray", "Bluray", "DVD", "WEB", "Remux"…
    pub resolution: Option<String>, // "2160p"
    pub codec: Option<String>,      // "HEVC"
    pub bit_depth: Option<u8>,
    pub hdr: Vec<String>,           // ["DV P7", "HDR10"]
    pub audio: Vec<AudioTrack>,
    pub langs: Option<String>,      // "MULTi", "VF", "VO", "VOSTFR"
    pub duration_min: Option<u32>,
    /// true si les infos viennent d'une analyse réelle (ffprobe)
    pub probed: bool,
    pub probe_error: Option<String>,
}

impl TechInfo {
    pub fn best_audio(&self) -> Option<&AudioTrack> {
        self.audio.iter().max_by_key(|a| a.score)
    }
    pub fn hdr_label(&self) -> String {
        self.hdr.join(" ")
    }
    pub fn audio_label(&self) -> String {
        match self.best_audio() {
            Some(a) if !a.channels.is_empty() => format!("{} {}", a.format, a.channels),
            Some(a) => a.format.clone(),
            None => String::new(),
        }
    }
}

/// Un résultat TMDB retenu.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TmdbMatch {
    pub id: u64,
    pub kind: MediaKind,
    pub title: String,
    pub original_title: String,
    pub original_language: String,
    pub year: Option<u16>,
    pub overview: String,
    pub poster_path: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    /// Pas encore analysé
    Pending,
    /// Identifié automatiquement avec une bonne confiance
    Auto,
    /// Plusieurs candidats / doute : à vérifier
    Review,
    /// Rien trouvé
    NotFound,
    /// Validé manuellement par l'utilisateur
    Confirmed,
    /// Déjà au bon nom
    UpToDate,
    /// Renommé pendant cette session
    Done,
    /// Erreur
    Error,
    /// Ne doit pas être renommé (voir MediaItem::locked)
    Locked,
}

impl Status {
    pub fn label(&self) -> &'static str {
        match self {
            Status::Pending => "En attente",
            Status::Auto => "Identifié",
            Status::Review => "À vérifier",
            Status::NotFound => "Non trouvé",
            Status::Confirmed => "Validé",
            Status::UpToDate => "Déjà conforme",
            Status::Done => "Renommé",
            Status::Error => "Erreur",
            Status::Locked => "Non renommable",
        }
    }
    pub fn css(&self) -> &'static str {
        match self {
            Status::Auto | Status::Confirmed => "success",
            Status::Review => "warning",
            Status::NotFound | Status::Error | Status::Locked => "error",
            Status::UpToDate | Status::Done => "accent",
            Status::Pending => "dim-label",
        }
    }
    pub fn ready(&self) -> bool {
        matches!(self, Status::Auto | Status::Confirmed)
    }
}

#[derive(Debug, Clone)]
pub struct MediaItem {
    /// Fichier vidéo, fichier .iso, ou dossier disque
    pub path: PathBuf,
    pub layout: Layout,
    pub kind: MediaKind,
    /// Dossier « catégorie » (Films, Séries, UHD…) dans lequel le média est rangé
    pub category_dir: PathBuf,
    pub guess: Guess,
    pub tech: TechInfo,
    pub matched: Option<TmdbMatch>,
    pub candidates: Vec<TmdbMatch>,
    /// Titre d'épisode TMDB (si épisode)
    pub episode_title: Option<String>,
    pub status: Status,
    pub included: bool,
    pub message: Option<String>,
    /// Raison pour laquelle ce média ne doit pas être renommé (structure disque mal placée…)
    pub locked: Option<String>,
}

impl MediaItem {
    /// Clé de regroupement : tous les épisodes d'une même série partagent leur identification.
    pub fn group_key(&self) -> String {
        match self.kind {
            MediaKind::Episode => format!(
                "tv:{}:{}",
                self.category_dir.display(),
                super::parse::normalize(&self.guess.title)
            ),
            MediaKind::Movie => format!("file:{}", self.path.display()),
        }
    }
    pub fn display_name(&self) -> String {
        self.path
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default()
    }
}
