//! Client TMDB minimal (API v3), bloquant : à appeler hors du thread de l'interface.

use super::model::{MediaKind, TmdbMatch};
use super::parse::normalize;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Mutex;

fn api() -> String {
    std::env::var("OHTPC_TMDB_API").unwrap_or_else(|_| "https://api.themoviedb.org/3".into())
}
fn img() -> String {
    std::env::var("OHTPC_TMDB_IMG").unwrap_or_else(|_| "https://image.tmdb.org/t/p/w185".into())
}

pub struct Tmdb {
    key: String,
    lang: String,
    agent: ureq::Agent,
    seasons: Mutex<HashMap<(u64, u16), HashMap<u16, String>>>,
}

impl Tmdb {
    pub fn new(key: &str, lang: &str) -> Self {
        let agent = ureq::Agent::config_builder()
            .timeout_global(Some(std::time::Duration::from_secs(20)))
            .build()
            .into();
        Self { key: key.trim().to_string(), lang: lang.to_string(), agent, seasons: Default::default() }
    }

    pub fn has_key(&self) -> bool {
        !self.key.is_empty()
    }

    fn get(&self, path: &str, params: &[(&str, String)]) -> Result<Value, String> {
        if self.key.is_empty() {
            return Err("Clé API TMDB manquante (Préférences)".into());
        }
        let mut req = self.agent.get(&format!("{}{path}", api())).query("language", &self.lang);
        // Jeton v4 (JWT) → en-tête Bearer, sinon clé v3 en paramètre
        if self.key.starts_with("eyJ") {
            req = req.header("Authorization", &format!("Bearer {}", self.key));
        } else {
            req = req.query("api_key", &self.key);
        }
        for (k, v) in params {
            req = req.query(*k, v);
        }
        let mut resp = req.call().map_err(|e| match e {
            ureq::Error::StatusCode(401) => "TMDB : clé API refusée (401)".to_string(),
            ureq::Error::StatusCode(429) => "TMDB : trop de requêtes, réessayez".to_string(),
            other => format!("TMDB : {other}"),
        })?;
        resp.body_mut().read_json::<Value>().map_err(|e| e.to_string())
    }

    fn parse_result(v: &Value, kind: MediaKind) -> TmdbMatch {
        let (t, ot, d) = match kind {
            MediaKind::Movie => ("title", "original_title", "release_date"),
            MediaKind::Episode => ("name", "original_name", "first_air_date"),
        };
        TmdbMatch {
            id: v["id"].as_u64().unwrap_or(0),
            kind,
            title: v[t].as_str().unwrap_or("").to_string(),
            original_title: v[ot].as_str().unwrap_or("").to_string(),
            original_language: v["original_language"].as_str().unwrap_or("").to_string(),
            year: v[d].as_str().and_then(|s| s.get(..4)).and_then(|y| y.parse().ok()),
            overview: v["overview"].as_str().unwrap_or("").to_string(),
            poster_path: v["poster_path"].as_str().map(String::from),
        }
    }

    pub fn search(&self, kind: MediaKind, query: &str, year: Option<u16>) -> Result<Vec<TmdbMatch>, String> {
        let (path, ykey) = match kind {
            MediaKind::Movie => ("/search/movie", "year"),
            MediaKind::Episode => ("/search/tv", "first_air_date_year"),
        };
        let mut params = vec![("query", query.to_string()), ("include_adult", "false".into())];
        if let Some(y) = year {
            params.push((ykey, y.to_string()));
        }
        let mut res = self.get(path, &params)?;
        let mut list: Vec<TmdbMatch> = res["results"]
            .as_array()
            .map(|a| a.iter().map(|v| Self::parse_result(v, kind)).collect())
            .unwrap_or_default();
        // Année parfois décalée d'un an (sortie festival / sortie FR) : on réessaie sans
        if list.is_empty() && year.is_some() {
            params.truncate(2);
            res = self.get(path, &params)?;
            list = res["results"]
                .as_array()
                .map(|a| a.iter().map(|v| Self::parse_result(v, kind)).collect())
                .unwrap_or_default();
        }
        list.truncate(12);
        Ok(list)
    }

    pub fn by_id(&self, kind: MediaKind, id: u64) -> Result<TmdbMatch, String> {
        let path = match kind {
            MediaKind::Movie => format!("/movie/{id}"),
            MediaKind::Episode => format!("/tv/{id}"),
        };
        let v = self.get(&path, &[])?;
        Ok(Self::parse_result(&v, kind))
    }

    /// Titres des épisodes d'une saison (mis en cache).
    pub fn episode_title(&self, tv_id: u64, season: u16, episode: u16) -> Option<String> {
        if let Some(s) = self.seasons.lock().unwrap().get(&(tv_id, season)) {
            return s.get(&episode).cloned();
        }
        let v = self.get(&format!("/tv/{tv_id}/season/{season}"), &[]).ok();
        let mut map = HashMap::new();
        if let Some(eps) = v.as_ref().and_then(|v| v["episodes"].as_array()) {
            for e in eps {
                if let (Some(n), Some(name)) = (e["episode_number"].as_u64(), e["name"].as_str()) {
                    map.insert(n as u16, name.to_string());
                }
            }
        }
        let r = map.get(&episode).cloned();
        self.seasons.lock().unwrap().insert((tv_id, season), map);
        r
    }

    pub fn poster(&self, path: &str) -> Option<Vec<u8>> {
        let mut r = self.agent.get(&format!("{}{path}", img())).call().ok()?;
        r.body_mut().read_to_vec().ok()
    }
}

/// Score de confiance d'un candidat (0–100).
pub fn score(m: &TmdbMatch, query: &str, year: Option<u16>) -> u32 {
    let q = normalize(query);
    let t = normalize(&m.title);
    let o = normalize(&m.original_title);
    let mut s: u32 = if q == t || q == o {
        70
    } else if t.starts_with(&q) || o.starts_with(&q) || q.starts_with(&t) {
        45
    } else {
        20
    };
    match (year, m.year) {
        (Some(a), Some(b)) if a == b => s += 30,
        (Some(a), Some(b)) if a.abs_diff(b) == 1 => s += 15,
        (Some(_), Some(_)) => s = s.saturating_sub(25),
        (None, _) => s += 5,
        _ => {}
    }
    s.min(100)
}

/// Choix automatique : un candidat clairement meilleur que les autres.
pub fn auto_pick(cands: &[TmdbMatch], query: &str, year: Option<u16>) -> Option<usize> {
    let mut scored: Vec<(usize, u32)> =
        cands.iter().enumerate().map(|(i, c)| (i, score(c, query, year))).collect();
    scored.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    let (best, bs) = *scored.first()?;
    let second = scored.get(1).map(|x| x.1).unwrap_or(0);
    // Sans année, un titre court est trop ambigu (« Heat », « Amélie »…) : on ne valide
    // automatiquement que s'il n'y a qu'un seul résultat.
    if year.is_none() {
        return (cands.len() == 1 && bs >= 70).then_some(best);
    }
    if (bs >= 95 && second < 95) || (bs >= 70 && bs >= second + 20) || (cands.len() == 1 && bs >= 45) {
        Some(best)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn m(id: u64, t: &str, y: u16) -> TmdbMatch {
        TmdbMatch { id, kind: MediaKind::Movie, title: t.into(), original_title: t.into(), original_language: "fr".into(), year: Some(y), overview: String::new(), poster_path: None }
    }
    #[test]
    fn auto_pick_rules() {
        let c = vec![m(1, "Le Fabuleux Destin d'Amélie Poulain", 2001), m(2, "Amélie", 2018)];
        assert_eq!(auto_pick(&c, "Amelie", None), None);
        let c = vec![m(1, "Le Parrain", 1972), m(2, "Le Parrain, 2e partie", 1974)];
        assert_eq!(auto_pick(&c, "Le Parrain", Some(1972)), Some(0));
        let c = vec![m(1, "Heat", 1995), m(2, "Heat", 1986)];
        assert_eq!(auto_pick(&c, "Heat", Some(1986)), Some(1));
    }
}
