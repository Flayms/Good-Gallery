# 004 · Metadata · Plan

Records the approach that was taken. Decisions that outlive this spec are in `docs/architecture.md`.

## Approach

- **Tags** (`domain::tags`): `clean_tag` (NFKC, trim, collapse whitespace) and `normalize_tag` (+ lower case). Whitespace is JavaScript's set (includes U+FEFF, excludes U+0085), because tag names in existing indexes were normalized with it. Category roots for People and Places.
- **exiftool** (`media::exiftool`): a pool of `exiftool -stay_open True -@ -` processes (size = parallel file reads). Each read uses the argument set of exiftool-vendored v38's defaults, which existing indexes were read with: `-charset filename=utf8 -json -fast`, a UTF-8 repair filter, `-api struct=1 -use MWG -api keepUTCTime`, numeric `-*Duration*# -GPS…# -Orientation#`, `-all <file> -ignoreMinorErrors`. Arguments go over stdin, one per line, no shell; paths are always absolute (REQ-011-09). A read over 60 s replaces its process. Executable: `GOOD_GALLERY_EXIFTOOL`, `exiftool\exiftool.exe` next to the app, then `PATH`.
- **Extraction** (`media::metadata`): dimensions swapped for orientations 5–8 and rotations 90 / 270; first valid date of `SubSecDateTimeOriginal`, `DateTimeOriginal`, `CreateDate`, `MediaCreateDate` (file before sidecar); duration; rating; tag paths from hierarchies, face regions and flat keywords (minus those already named; numbers kept as text). File and sidecar are read in parallel. Values of unexpected types are ignored, never panic (REQ-011-10).
- **Face regions** (REQ-004-09): with `struct=1` exiftool returns MWG and Microsoft regions as structs (`RegionInfo.RegionList[]` of type `Face`, `RegionInfoMP.Regions[]`); both are read. Earlier versions only looked at flattened fields and missed them, so files with regions get `People|<name>` tags on their next full scan.
- **Dates** (REQ-004-03): an offset in the string wins; otherwise the zone from `OffsetTimeOriginal` / `OffsetTimeDigitized` / `TimeZone`, or inferred from another capture date that carries an offset (incl. Nikon daylight saving), or the offset to the GPS timestamp; QuickTime video dates are UTC; otherwise the computer's local zone.
- **`METADATA_VERSION`** stays 1 while the rules don't change; bumping it re-reads every file once (REQ-002-05).

## Deviations

- **No GPS-based time zones.** Photos with GPS but no offset tags, no zoned capture date and no GPS timestamp get the computer's local zone instead of the zone at the GPS position (that would need a time-zone shape database such as geo-tz). REQ-004-03 stays partial.

## Tasks

- [x] Tag cleaning, normalization, category roots (REQ-004-11)
- [x] exiftool process pool, arguments, timeouts, executable lookup (REQ-011-09)
- [x] Extraction rules (REQ-004-02, -04 … -09, REQ-011-10)
- [x] Dates and time zones (REQ-004-03, partial)
- [x] Sidecars: lookup rule and merge (REQ-004-10)
- [x] Writer: media upsert, tag paths, pruning, folders (REQ-004-07, -08)
- [ ] Time zone from the GPS position, if it matters in practice (REQ-004-03)
