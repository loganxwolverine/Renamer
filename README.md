# OpenHTPC Renamer

Application de bureau GTK4 / libadwaita en Rust qui renomme une médiathèque
(films, séries, dossiers DVD, Blu-ray, UHD, images ISO). Les noms produits
permettent à OpenHTPC (et à tout scraper TMDB) d'identifier chaque média sans
ambiguïté. Ils contiennent aussi les vraies informations techniques.

```
Films/Le Parrain/                                   ← vos dossiers restent tels quels
    Le Parrain (1972) [The Godfather] [tmdbid-238] - UHD Bluray 2160p HEVC DV P7 HDR10 TrueHD Atmos 7.1 MULTi.mkv
    Le Parrain (1972) [The Godfather] [tmdbid-238] - UHD Bluray 2160p HEVC DV P7 HDR10 TrueHD Atmos 7.1 MULTi.fr.srt
Séries/Breaking Bad/Season 1/
    Breaking Bad - S01E01 - Chute libre - Bluray 1080p HEVC DTS-HD MA 5.1 MULTi.mkv
```

## Installation (Fedora)

```bash
./install.sh
```

Ou à la main :

```bash
sudo dnf install rust cargo gtk4-devel libadwaita-devel ffmpeg-free
cargo build --release
./target/release/openhtpc-renamer
```

Au premier lancement : **Menu → Préférences → Clé API TMDB**. La clé v3
gratuite et le jeton de lecture v4 sont tous deux acceptés.

## Fonctionnement

Par défaut, **seuls les noms changent** : aucun dossier n'est créé, déplacé ou
supprimé. Un dossier disque (`VIDEO_TS`, `BDMV`) est renommé à sa place. Le
rangement automatique en `Film (année)/` et `Série/Saison XX/` est une option
(Préférences → « Réorganiser l'arborescence »).

1. **Ouvrir** le dossier de la médiathèque (Ctrl+O).
2. **Scan.** L'application repère :
   - les fichiers vidéo ;
   - les dossiers `VIDEO_TS` (DVD) et `BDMV` (Blu-ray / UHD) : c'est le dossier qu'on renomme, jamais son contenu ;
   - les `.iso` ;
   - les épisodes (`S01E02`, `1x02`, `Saison 2/03 - ….mkv`).

   Les noms vides de sens comme `title_t00.mkv` (MakeMKV) ou `DISC1` sont remplacés par le nom du dossier parent.
3. **Analyse technique** avec `ffprobe` (pas d'après le nom) :
   - résolution, codec, 10 bits ;
   - HDR10, HDR10+, HLG, Dolby Vision avec son profil ;
   - TrueHD Atmos, DTS-X, DTS-HD MA, DDP Atmos, canaux ;
   - langues : MULTi, VF, VO, VOSTFR.

   Pour un Blu-ray, c'est le plus gros `.m2ts` qui est analysé ; au-delà de 2160p, la source devient « UHD Bluray ».
4. **Identification TMDB** (titres en français, titre original conservé) :
   - **Identifié** : correspondance certaine (titre + année).
   - **À vérifier** : plusieurs candidats. Cliquez le bon, avec affiche et résumé. Vous pouvez aussi corriger le titre ou l'année, ou saisir l'ID TMDB.
   - Pour une série, un choix s'applique à tous ses épisodes.
   - Un `[tmdbid-123]` déjà présent dans un nom est réutilisé directement.
5. **Renommer** : un aperçu avant/après s'affiche et rien n'est fait sans confirmation.
   - Les sous-titres, `.nfo` et images portant le même nom suivent le fichier.
   - Les anciens dossiers devenus vides sont supprimés.
   - Aucun fichier existant n'est jamais écrasé.
6. **Annuler** (Ctrl+Z) : chaque renommage est journalisé dans
   `~/.local/share/openhtpc-renamer/journal/` et peut être annulé.

## Le partage SMB (smb://192.168.1.10/Media)

Deux possibilités :

- **Simple.** Ouvrez le partage dans Fichiers (Nautilus), puis choisissez-le
  dans l'application. Il apparaît sous `/run/user/1000/gvfs/smb-share:server=192.168.1.10,share=media/`.
- **Plus rapide et plus fiable** (recommandé pour une grosse bibliothèque) : montage CIFS.
  ```bash
  sudo dnf install cifs-utils
  sudo mkdir -p /mnt/media
  sudo mount -t cifs //192.168.1.10/Media /mnt/media -o username=VOTRE_USER,uid=$(id -u),gid=$(id -g),iocharset=utf8
  ```
  Pour un montage permanent, ajoutez la ligne équivalente dans `/etc/fstab` avec
  un fichier `credentials=`.

Sur un réseau lent, réduisez « Analyses en parallèle » à 1 ou 2.

## Modèles de noms

Ils se modifient dans **Préférences → Modèles de noms**, avec un aperçu en direct.

| Variable | Exemple |
|---|---|
| `{titre}` / `{serie}` | Le Parrain |
| `{titre_original}` | The Godfather (vide s'il est identique au titre) |
| `{annee}` `{tmdbid}` | 1972, 238 |
| `{saison}` `{episode}` `{titre_episode}` | 01, 05 (ou `05-E06`), Chute libre |
| `{source}` | DVD, Bluray, UHD Bluray, Remux, WEB-DL… |
| `{resolution}` `{codec}` `{bits}` | 2160p, HEVC, 10bit |
| `{hdr}` | DV P7 HDR10, HDR10+, HLG |
| `{audio}` | TrueHD Atmos 7.1, DTS-HD MA 5.1 |
| `{langues}` | MULTi, VF, VO, VOSTFR |
| `{tech}` | la composition de toutes les infos techniques |

Une partie entre `< >` disparaît si l'une de ses variables est vide :
`{titre}< [{titre_original}]>`.

**Pour OpenHTPC** : l'identifiant TMDB se lit dans le nom avec l'expression
`\[tmdbid-(\d+)\]`. Il suffit d'appeler `GET /movie/{id}` ou `GET /tv/{id}`,
sans aucune recherche.

## Mode texte (aperçu sans rien modifier)

```bash
openhtpc-renamer --apercu /mnt/media            # avec TMDB
openhtpc-renamer --apercu /mnt/media --sans-tmdb
```

## Développement

```bash
cargo test
```

- `src/core/`
  - `parse.rs` : analyse des noms
  - `probe.rs` : ffprobe
  - `tmdb.rs` : client TMDB
  - `naming.rs` : modèles et plan de renommage
  - `rename.rs` : exécution, journal, annulation
  - `scan.rs`, `engine.rs` : scan et chaîne d'analyse
- `src/ui/` : interface GTK4 / libadwaita.
