# Aurivon product research (2024–2026)

Qualitative synthesis from community sources (Plex, Jellyfin, Stremio, Kodi, Infuse discussions and issue trackers), mapped to Aurivon as of November 2026.

**Positioning bet:** unified desktop hybrid (local library + stream discovery + Trakt continuity + mpv), not a Stremio clone or Plex server.

Audience: **A** local-first · **B** stream/debrid-first · **C** hybrid (primary target).

---

## High themes vs Aurivon today

| # | Theme | Status | Notes |
|---|--------|--------|--------|
| 1 | Unified discovery and playback | **Partial** | Details shows local Play + streams; Library vs Lists still feel like two apps. Title page should lead with one identity + all sources. |
| 2 | Accurate local metadata matching | **Partial** | TMDB match + Re-match + Matched/Unmatched filter. Missing: pick match UI, lock overrides, protect against rematch clobber. |
| 3 | Continue watching / history | **Partial** | Local progress + Trakt playback merge; gaps on conflict rules, history import, trust. |
| 4 | Local playback quality | **Have** | Embedded mpv; advanced settings exist but diagnostics (HDR/out) thin. |
| 5 | Reliable addons / providers | **Partial** | Torrentio + HTTP addons; soft-fail warnings added. Need per-provider status, retry, never block title page. |
| 6 | Fast incremental scanning | **Partial** | Scan on add/rescan; no file watcher, weak progress UX for large libs. |
| 7 | Trakt beyond scrobble | **Partial / strong** | Lists hub, ratings, favorites, collection, calendar, hide. Gaps: first-connect history import, clearer sync health. |
| 8 | Subtitle quality and control | **Partial** | OpenSubtitles search; missing dual subs, forced auto, offset UI polish, preference memory. |
| 9 | Customizable Home rows | **Partial** | Row toggles in Settings; no reorder, limited Trakt-list-as-row. |
| 10 | Universal search | **Partial** | Local + TMDB search; not one ranked mix with clear source chips for every hit. |
| 11 | Up Next / autoplay / intro skip | **Have / partial** | Autoplay countdown, intro segments; credits/outro UX thinner. |
| 12 | Lists / collections / watchlists | **Partial** | Lists hub + Trakt; local collections thin. |

Architecture rule from research (keep): **content identity (TMDB) ≠ playback source**. Local file and addon stream are two sources on one title. Offline library must work without internet/addons.

---

## Next backlog (discussion order)

### P0 (1–2 months): reliability of the hybrid core

Status: **shipped in Unreleased** (desktop). Remaining polish is P1+.

1. **Title page as hub** — done  
   Details **Sources** section (on-disk path + stream list); availability badges; Trakt progress; streams soft-fail with warnings (page stays usable).

2. **Library matching UX** — done  
   Match / Fix match / Lock match on cards and Details; `media_links.match_locked` skips force rematch.

3. **Continue Watching trust** — done  
   Local first, then Trakt-only with “Continue · Trakt”; sync pulls farther Trakt playback into local progress; history/recommendations rows when catalogs enabled.

4. **Addon / Torrentio resilience** — done  
   Stream search warnings, Retry, Enable Torrentio from empty state; toggle addons without killing the search path.

5. **Scan UX** — done  
   Live scan progress in busy banner; missing/disconnected library folder called out in Settings.

6. **Home configurability** — done  
   Settings → Home rows: ↑↓ reorder, hide/add, pin Trakt custom lists as `trakt_list:<id>` rows.

### P1 (later)

- Subtitle offset / forced / dual / remember prefs — **shipped (Unreleased)**  
  Player Subs: ±0.5s timing (z/x), Forced only, secondary track; prefs remembered in config.  
- HDR / hwdec diagnostics in player — **shipped (Unreleased)**  
  Settings hwdec (auto/no/vaapi/nvdec/d3d11va); Speed panel shows live Decode + Video (HDR tags).  
- Hide anime filter — **shipped (Unreleased)**  
  Settings / Library: hide Japanese animation from discovery surfaces (not an anime catalog).  
- 10-foot / controller focus mode — **shipped (Unreleased)**  
  Settings → Appearance **Couch mode**: larger cards; Library ←→↑↓ focus + Enter.  
- Smart collections — **shipped (Unreleased)**  
  Home rows `smart:unwatched` / `smart:recent` / `smart:uhd` (on-device virtual shelves).  

**Earlier roadmap (storage → watchlist → library → addons → binge/home):** shipped through 0.6–1.0 / Unreleased.

### Explicitly skip (v1)

- Android / APK  
- Self-hosted media server  
- Multi-user profiles / parental controls  
- Live TV / DVR  
- Music / photos as first-class  
- **Anime as a product lane** (AniList IDs, absolute episode numbering, anime-first UX). Optional AniList sync may remain; the only first-class anime control is **Hide anime**.  

---

## Open questions (owner)

1. Primary UI: desktop mouse/keyboard, TV couch, or both?  
2. Initial audience: hybrid, Stremio refugees, or library collectors?  
3. Is a 30k-file library a first-class target?  
4. Stremio protocol only vs native addon SDK?  
5. Anime depth for v1? **No** — hide filter only.  

6. Playback: compete with Kodi/Infuse quality or “convenient mpv defaults”?  
7. Trakt: local state primary + sync, or Trakt canonical?  
8. How custom should discovery rows get?

**Default answers for planning (until overridden):** hybrid audience; local library first-class; Stremio-compatible addons + builtins; local watch state primary with Trakt sync; desktop keyboard/mouse first, couch later; **not an anime app** (hide-anime filter only).

---

## Sources (research snapshot)

Qualitative: r/PleX, r/jellyfin, r/StremioAddons, r/htpc; Stremio / Jellyfin / Jellyfin Desktop GitHub; Plex / Trakt / Infuse / Kodi docs (2024–2026). Not a representative survey.
