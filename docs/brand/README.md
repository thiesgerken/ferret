# Brand

ferret's mark is a ferret rearing up to look: head in profile, small low ear, the eye-mask, and a
straight neck that ends on a flat base, so the mark sits on a text baseline. The face reads
curious, not fierce.

## Files

| file | what it is |
| --- | --- |
| `mark.svg` | the mark, 32×32 viewBox, switches colours with `prefers-color-scheme` |
| `mark-light.svg` / `mark-dark.svg` | the same mark with fixed colours |
| `mark-mono.svg` | one colour via `currentColor`; the mask is a hole, the eye a dot |
| `mark-512.png` | raster export of the light mark |

The favicon set is in `frontend/src/app/` (`icon.svg`, `favicon.ico` with 16/32/48 frames,
`apple-icon.png` 180×180), where Next.js picks it up by filename.

## Colours

| role | light theme | dark theme |
| --- | --- | --- |
| body | `oklch(0.5 0.09 60)` ≈ `#885629` | `oklch(0.72 0.1 65)` ≈ `#d0975f` |
| eye mask | `#4d2d15` | `#4d2d15` |
| pupil / catch-light | `#1c110a` / `#f3ece2` | same |
| apple icon | tile `#885629`, ferret `#f3ece2`, mask and pupil as above | — |

The body colour is the UI's `--brand`.

## Wordmark

"ferret" is set in Alegreya SemiBold, lowercase, in `--brand` (Tailwind's `font-wordmark`). Next to the
mark it sits on the mark's flat base, with the mark at 0.8 em.

## Using it

Use `mark.svg` where the page follows the system colour scheme, and the fixed variants where it
doesn't (emails, badges, an element with its own background). `mark-mono.svg` takes the
surrounding `color`, for toolbars next to icons.

Keep clear space of at least the ear height around the mark. It holds up to 16 px. Don't rotate
it or outline it.
