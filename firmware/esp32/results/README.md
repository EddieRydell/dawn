# ESP32 evidence index

Result files are committed evidence, not build output. A capture is accepted only
when its collector completed, checksums matched, and the report says so. Never
repair a corrupt serial capture by dropping bytes.

## Current accepted evidence

- `2026-09-06-audit-i2s-verified.txt`: final loader image; Wi-Fi upload checks,
  200 frame CRC checks, and 13,080 I2S playback frames with no missed deadline.
- `2026-09-06-audit-final-*.txt`: six final-image, controller-shaped Wi-Fi
  fixtures. All 576 requested frame CRCs matched with zero evaluation allocations.

## Qualified and historical evidence

- `2026-09-06-audit-pc-drained.txt`: accepted PC profile from an intermediate
  image; it predates final mark broadcast and hue changes.
- `2026-09-06-audit-i2s-retry.txt` and `2026-09-06-audit-wifi-mark-*.txt`:
  diagnostic runs superseded by the final verified loader capture.
- Dated 2026-09-04 and 2026-09-05 files record earlier optimization work. They
  remain useful for provenance but are not current performance claims.

## Failed attempts

`2026-09-06-audit-i2s.txt` is incomplete: it timed out while checking a malformed
body. `2026-09-06-audit-pc*.txt` files other than `audit-pc-drained.txt` failed
strict serial validation. They are retained to document the limitation, not to
support timing or profile claims.
