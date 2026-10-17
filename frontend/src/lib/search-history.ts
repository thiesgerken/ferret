import { searchHref, type SearchState } from "@/lib/search-url";

/*
 * `history.pushState` keeps the page and updates `useSearchParams`, Next's router follows it.
 *
 * A query that's typed but not submitted lives in one history entry marked `typing`: the first
 * pause in typing pushes it, later ones replace it, so Back returns to the last submitted query
 * instead of stepping through every prefix.
 */

/** A search the user asked for: Enter, a chip, a filter. */
export function submitSearch(state: SearchState) {
  const href = searchHref(state);
  if (href === window.location.pathname + window.location.search) {
    // Enter on what typing already shows: it's submitted now, so the next typing pushes again.
    window.history.replaceState({ ...window.history.state, typing: false }, "", href);
  } else {
    window.history.pushState(null, "", href);
  }
}

/** A query typed into the field, once typing pauses. */
export function typeSearch(state: SearchState) {
  const href = searchHref(state);
  if (window.history.state?.typing) {
    window.history.replaceState(window.history.state, "", href);
  } else {
    window.history.pushState({ typing: true }, "", href);
  }
}
