# Changelog

## [0.1.0-rc.1] - 2026-11-02

Première version candidate, testée sur une médiathèque réelle.

### Fonctionnalités
- Scan des films, épisodes, images ISO, dossiers DVD (`VIDEO_TS`) et Blu-ray / UHD (`BDMV`).
- Analyse technique réelle avec ffprobe :
  - résolution, codec, profondeur de couleur ;
  - HDR10, HDR10+, HLG, Dolby Vision avec son profil ;
  - TrueHD Atmos, DTS-X, DTS-HD MA, DDP Atmos, canaux ;
  - langues : MULTi, VF, VO, VOSTFR.
- Identification TMDB :
  - titres français et titre original, affiches ;
  - validation automatique seulement si elle est sûre ;
  - un choix par série s'applique à tous ses épisodes ;
  - réutilisation d'un `[tmdbid-…]` déjà présent dans le nom.
- **Renommage sur place par défaut** : l'arborescence n'est pas modifiée. La réorganisation en dossiers est disponible en option.
- Modèles de noms configurables, avec aperçu en direct.
- Aperçu avant/après, aucun écrasement, journal et annulation complète (Ctrl+Z).
- Protection des structures disque :
  - aucun fichier interne n'est renommé ;
  - un `BDMV` ou `VIDEO_TS` mal placé est signalé « Non renommable » ;
  - les disques sont numérotés (`- Disque 2`).
- Mode texte sans modification : `openhtpc-renamer --apercu <dossier>`.
