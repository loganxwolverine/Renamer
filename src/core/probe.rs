//! Analyse technique réelle via `ffprobe` (paquet ffmpeg-free ou ffmpeg sur Fedora).

use super::model::{AudioTrack, Layout, TechInfo};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Command;
use walkdir::WalkDir;

/// Choisit le fichier à analyser : la vidéo elle-même, ou le plus gros flux d'un disque.
pub fn probe_target(path: &Path, layout: Layout) -> Option<PathBuf> {
    match layout {
        Layout::File => Some(path.to_path_buf()),
        Layout::Iso => None,
        Layout::BlurayFolder => largest(&path.join("BDMV").join("STREAM"), &["m2ts", "mts"]),
        Layout::DvdFolder => largest(&path.join("VIDEO_TS"), &["vob"]),
    }
}

fn largest(dir: &Path, exts: &[&str]) -> Option<PathBuf> {
    WalkDir::new(dir)
        .max_depth(1)
        .into_iter()
        .flatten()
        .filter(|e| e.file_type().is_file())
        .filter(|e| {
            e.path()
                .extension()
                .map(|x| exts.contains(&x.to_string_lossy().to_lowercase().as_str()))
                .unwrap_or(false)
        })
        .max_by_key(|e| e.metadata().map(|m| m.len()).unwrap_or(0))
        .map(|e| e.into_path())
}

fn run_ffprobe(args: &[&str], file: &Path) -> Result<Value, String> {
    let out = Command::new("ffprobe")
        .args(["-v", "error", "-print_format", "json"])
        .args(args)
        .arg(file)
        .output()
        .map_err(|e| format!("ffprobe introuvable ({e}). Installez : sudo dnf install ffmpeg-free"))?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    serde_json::from_slice(&out.stdout).map_err(|e| e.to_string())
}

pub fn probe(path: &Path, layout: Layout) -> TechInfo {
    let mut t = TechInfo::default();
    match layout {
        Layout::DvdFolder => t.source = Some("DVD".into()),
        Layout::BlurayFolder => t.source = Some("Bluray".into()),
        _ => {}
    }
    let Some(target) = probe_target(path, layout) else {
        if layout == Layout::Iso {
            t.probe_error = Some("Image ISO : analyse impossible, infos déduites du nom".into());
        } else {
            t.probe_error = Some("Aucun flux vidéo trouvé dans le disque".into());
        }
        return t;
    };
    let v = match run_ffprobe(&["-show_streams", "-show_format"], &target) {
        Ok(v) => v,
        Err(e) => {
            t.probe_error = Some(e);
            return t;
        }
    };
    fill_from_json(&mut t, &v);

    // HDR10+ : métadonnées dynamiques présentes uniquement dans les trames
    let pq = v["streams"]
        .as_array()
        .map(|a| a.iter().any(|s| s["color_transfer"] == "smpte2084"))
        .unwrap_or(false);
    if pq && !t.hdr.iter().any(|h| h == "HDR10+") {
        if let Ok(f) = run_ffprobe(
            &["-select_streams", "v:0", "-read_intervals", "%+#3", "-show_frames", "-show_entries", "frame=side_data_list"],
            &target,
        ) {
            let s = f.to_string();
            if s.contains("SMPTE2094-40") || s.contains("HDR10+") {
                t.hdr.retain(|h| h != "HDR10");
                t.hdr.push("HDR10+".into());
            }
        }
    }
    if layout == Layout::BlurayFolder && t.resolution.as_deref() == Some("2160p") {
        t.source = Some("UHD Bluray".into());
    }
    t.probed = true;
    t
}

/// Remplit TechInfo depuis la sortie JSON de ffprobe (séparé pour les tests).
pub fn fill_from_json(t: &mut TechInfo, v: &Value) {
    let empty = vec![];
    let streams = v["streams"].as_array().unwrap_or(&empty);

    if let Some(d) = v["format"]["duration"].as_str().and_then(|d| d.parse::<f64>().ok()) {
        t.duration_min = Some((d / 60.0).round() as u32);
    }

    let videos: Vec<&Value> = streams
        .iter()
        .filter(|s| s["codec_type"] == "video" && s["disposition"]["attached_pic"] != 1)
        .collect();
    if let Some(vs) = videos.first() {
        let w = vs["width"].as_u64().unwrap_or(0);
        let h = vs["height"].as_u64().unwrap_or(0);
        t.resolution = resolution_label(w, h, vs["field_order"].as_str());
        t.codec = Some(codec_label(vs["codec_name"].as_str().unwrap_or("")));
        let pix = vs["pix_fmt"].as_str().unwrap_or("");
        t.bit_depth = vs["bits_per_raw_sample"]
            .as_str()
            .and_then(|b| b.parse().ok())
            .or(if pix.contains("10") { Some(10) } else if pix.contains("12") { Some(12) } else { Some(8) });

        // Dolby Vision : enregistrement de configuration DOVI sur n'importe quel flux vidéo
        // (sur UHD Blu-ray la couche d'amélioration est un second flux)
        let mut dv_profile = None;
        for s in &videos {
            if let Some(sd) = s["side_data_list"].as_array() {
                for d in sd {
                    if d["side_data_type"].as_str().map(|x| x.contains("DOVI")).unwrap_or(false) {
                        dv_profile = d["dv_profile"].as_u64().or(Some(0));
                    }
                }
            }
            let tag = s["codec_tag_string"].as_str().unwrap_or("");
            if ["dvhe", "dvh1", "dav1", "dva1", "dvav"].contains(&tag) && dv_profile.is_none() {
                dv_profile = Some(0);
            }
        }
        if let Some(p) = dv_profile {
            t.hdr.push(if p > 0 { format!("DV P{p}") } else { "DV".into() });
        }
        match vs["color_transfer"].as_str().unwrap_or("") {
            "smpte2084" => {
                // DV profil 5 n'a pas de couche HDR10 compatible
                if dv_profile != Some(5) {
                    t.hdr.push("HDR10".into());
                }
            }
            "arib-std-b67" => t.hdr.push("HLG".into()),
            _ => {}
        }
        if let Some(sd) = vs["side_data_list"].as_array() {
            if sd.iter().any(|d| d.to_string().contains("2094-40")) {
                t.hdr.retain(|h| h != "HDR10");
                t.hdr.push("HDR10+".into());
            }
        }
    }

    let mut langs: Vec<String> = vec![];
    for s in streams.iter().filter(|s| s["codec_type"] == "audio") {
        let lang = s["tags"]["language"].as_str().map(|l| l.to_lowercase()).filter(|l| l != "und");
        if let Some(l) = &lang {
            langs.push(l.clone());
        }
        let ch = s["channels"].as_u64().unwrap_or(0);
        let (format, score) = audio_label(
            s["codec_name"].as_str().unwrap_or(""),
            s["profile"].as_str().unwrap_or(""),
            s["tags"]["title"].as_str().unwrap_or(""),
        );
        t.audio.push(AudioTrack {
            format,
            channels: channels_label(ch, s["channel_layout"].as_str().unwrap_or("")),
            lang,
            score: score * 10 + ch as u32,
        });
    }
    let fr_sub = streams.iter().any(|s| {
        s["codec_type"] == "subtitle" && is_french(s["tags"]["language"].as_str().unwrap_or(""))
    });
    t.langs = langs_label(&langs, fr_sub);
}

pub fn is_french(l: &str) -> bool {
    matches!(l.to_lowercase().as_str(), "fre" | "fra" | "fr" | "fr-fr" | "fr-ca" | "french")
}

pub fn langs_label(langs: &[String], fr_sub: bool) -> Option<String> {
    if langs.is_empty() {
        return None;
    }
    let mut uniq: Vec<&str> = langs.iter().map(|s| s.as_str()).collect();
    uniq.sort();
    uniq.dedup();
    let has_fr = uniq.iter().any(|l| is_french(l));
    let other = uniq.iter().any(|l| !is_french(l));
    Some(
        match (has_fr, other) {
            (true, true) => "MULTi",
            (true, false) => "VF",
            (false, _) if fr_sub => "VOSTFR",
            _ => "VO",
        }
        .into(),
    )
}

pub fn resolution_label(w: u64, h: u64, field_order: Option<&str>) -> Option<String> {
    if w == 0 || h == 0 {
        return None;
    }
    let interlaced = matches!(field_order, Some("tt" | "bb" | "tb" | "bt"));
    // on raisonne surtout sur la largeur (films recadrés en 2.39:1)
    let p = if w >= 3200 || h >= 1800 {
        "2160"
    } else if w >= 1700 || h >= 1000 {
        "1080"
    } else if w >= 1200 || h >= 700 {
        "720"
    } else if h > 480 {
        "576"
    } else {
        "480"
    };
    Some(format!("{p}{}", if interlaced && p != "2160" { "i" } else { "p" }))
}

pub fn codec_label(c: &str) -> String {
    match c {
        "hevc" | "h265" => "HEVC",
        "h264" | "avc" => "H264",
        "av1" => "AV1",
        "vp9" => "VP9",
        "mpeg2video" => "MPEG2",
        "mpeg4" => "MPEG4",
        "vc1" => "VC1",
        other => return other.to_uppercase(),
    }
    .into()
}

/// (libellé, score de qualité)
pub fn audio_label(codec: &str, profile: &str, title: &str) -> (String, u32) {
    let p = profile.to_lowercase();
    let atmos = p.contains("atmos") || title.to_lowercase().contains("atmos");
    match codec {
        "truehd" if atmos => ("TrueHD Atmos".into(), 90),
        "truehd" => ("TrueHD".into(), 80),
        "dts" if p.contains("dts:x") || p.contains("dts-x") => ("DTS-X".into(), 88),
        "dts" if p.contains("ma") => ("DTS-HD MA".into(), 78),
        "dts" if p.contains("hra") || p.contains("hi res") => ("DTS-HD HRA".into(), 60),
        "dts" if p.contains("es") => ("DTS-ES".into(), 45),
        "dts" => ("DTS".into(), 40),
        "eac3" if atmos => ("DDP Atmos".into(), 70),
        "eac3" => ("DDP".into(), 50),
        "ac3" => ("AC3".into(), 30),
        "flac" => ("FLAC".into(), 75),
        c if c.starts_with("pcm_") => ("LPCM".into(), 76),
        "aac" => ("AAC".into(), 20),
        "opus" => ("Opus".into(), 22),
        "mp3" => ("MP3".into(), 10),
        "mp2" => ("MP2".into(), 8),
        other => (other.to_uppercase(), 5),
    }
}

pub fn channels_label(ch: u64, layout: &str) -> String {
    if layout.contains("7.1") {
        return "7.1".into();
    }
    if layout.contains("5.1") {
        return "5.1".into();
    }
    match ch {
        0 => String::new(),
        1 => "1.0".into(),
        2 => "2.0".into(),
        3 => "2.1".into(),
        6 => "5.1".into(),
        7 => "6.1".into(),
        8 => "7.1".into(),
        n => format!("{n}ch"),
    }
}

/// Secours : infos déduites des tags du nom de fichier.
pub fn fill_from_tags(t: &mut TechInfo, tags: &[String]) {
    let has = |xs: &[&str]| tags.iter().any(|t| xs.contains(&t.as_str()));
    if t.resolution.is_none() {
        t.resolution = if has(&["2160p", "4k", "uhd"]) {
            Some("2160p".into())
        } else if has(&["1080p"]) {
            Some("1080p".into())
        } else if has(&["1080i"]) {
            Some("1080i".into())
        } else if has(&["720p"]) {
            Some("720p".into())
        } else if has(&["576p"]) {
            Some("576p".into())
        } else if has(&["480p"]) {
            Some("480p".into())
        } else {
            None
        };
    }
    if t.source.is_none() {
        t.source = if has(&["remux", "bdremux"]) {
            Some(if t.resolution.as_deref() == Some("2160p") { "UHD Remux" } else { "Remux" }.into())
        } else if has(&["bluray", "blu-ray", "bdrip", "brrip", "bd", "bd25", "bd50", "bd66", "bd100"]) {
            Some(if t.resolution.as_deref() == Some("2160p") { "UHD Bluray" } else { "Bluray" }.into())
        } else if has(&["web-dl", "webdl", "web"]) {
            Some("WEB-DL".into())
        } else if has(&["webrip"]) {
            Some("WEBRip".into())
        } else if has(&["hdtv"]) {
            Some("HDTV".into())
        } else if has(&["dvd", "dvdrip", "dvd5", "dvd9", "dvdr"]) {
            Some("DVD".into())
        } else {
            None
        };
    }
    if t.codec.is_none() {
        t.codec = if has(&["x265", "h265", "hevc", "h.265"]) {
            Some("HEVC".into())
        } else if has(&["x264", "h264", "avc", "h.264"]) {
            Some("H264".into())
        } else if has(&["av1"]) {
            Some("AV1".into())
        } else {
            None
        };
    }
    if t.hdr.is_empty() {
        if has(&["dv", "dovi"]) {
            t.hdr.push("DV".into());
        }
        if has(&["hdr10+", "hdr10plus"]) {
            t.hdr.push("HDR10+".into());
        } else if has(&["hdr", "hdr10"]) {
            t.hdr.push("HDR10".into());
        }
    }
    if t.audio.is_empty() {
        let f = if has(&["truehd"]) && has(&["atmos"]) {
            Some("TrueHD Atmos")
        } else if has(&["truehd"]) {
            Some("TrueHD")
        } else if has(&["dts-x", "dtsx"]) {
            Some("DTS-X")
        } else if has(&["dts-hd", "dtshd"]) {
            Some("DTS-HD MA")
        } else if has(&["dts"]) {
            Some("DTS")
        } else if has(&["ddp", "ddp5", "eac3", "dd+"]) {
            Some(if has(&["atmos"]) { "DDP Atmos" } else { "DDP" })
        } else if has(&["ac3", "dd", "dd5"]) {
            Some("AC3")
        } else if has(&["aac"]) {
            Some("AAC")
        } else {
            None
        };
        if let Some(f) = f {
            t.audio.push(AudioTrack { format: f.into(), score: 1, ..Default::default() });
        }
    }
    if t.langs.is_none() {
        t.langs = if has(&["multi", "multi-vf2"]) {
            Some("MULTi".into())
        } else if has(&["vostfr", "subfrench"]) {
            Some("VOSTFR".into())
        } else if has(&["french", "truefrench", "vff", "vfq", "vf", "vfi", "vf2"]) {
            Some("VF".into())
        } else {
            None
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn uhd_dv_atmos() {
        let v = json!({
            "format": {"duration": "7200.0"},
            "streams": [
                {"codec_type":"video","codec_name":"hevc","width":3840,"height":1600,"pix_fmt":"yuv420p10le",
                 "color_transfer":"smpte2084",
                 "side_data_list":[{"side_data_type":"DOVI configuration record","dv_profile":8}]},
                {"codec_type":"audio","codec_name":"truehd","profile":"Dolby TrueHD + Dolby Atmos","channels":8,"channel_layout":"7.1","tags":{"language":"eng"}},
                {"codec_type":"audio","codec_name":"eac3","channels":6,"tags":{"language":"fre"}},
                {"codec_type":"subtitle","codec_name":"subrip","tags":{"language":"fre"}}
            ]
        });
        let mut t = TechInfo::default();
        fill_from_json(&mut t, &v);
        assert_eq!(t.resolution.as_deref(), Some("2160p"));
        assert_eq!(t.codec.as_deref(), Some("HEVC"));
        assert_eq!(t.hdr, vec!["DV P8", "HDR10"]);
        assert_eq!(t.audio_label(), "TrueHD Atmos 7.1");
        assert_eq!(t.langs.as_deref(), Some("MULTi"));
        assert_eq!(t.duration_min, Some(120));
        assert_eq!(t.bit_depth, Some(10));
    }

    #[test]
    fn dvd_like() {
        let v = json!({"streams":[
            {"codec_type":"video","codec_name":"mpeg2video","width":720,"height":576,"field_order":"tt"},
            {"codec_type":"audio","codec_name":"ac3","channels":6,"tags":{"language":"fre"}}
        ]});
        let mut t = TechInfo::default();
        fill_from_json(&mut t, &v);
        assert_eq!(t.resolution.as_deref(), Some("576i"));
        assert_eq!(t.audio_label(), "AC3 5.1");
        assert_eq!(t.langs.as_deref(), Some("VF"));
    }

    #[test]
    fn dts_variants() {
        assert_eq!(audio_label("dts", "DTS-HD MA + DTS:X", "").0, "DTS-X");
        assert_eq!(audio_label("dts", "DTS-HD MA", "").0, "DTS-HD MA");
        assert_eq!(audio_label("eac3", "Dolby Digital Plus + Dolby Atmos", "").0, "DDP Atmos");
    }
}
