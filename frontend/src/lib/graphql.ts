import { Client, cacheExchange, fetchExchange, mapExchange, type CombinedError } from "urql";

/** Dispatched on `window` whenever a GraphQL request comes back 401: the session is gone. */
export const SESSION_ENDED_EVENT = "ferret:session-ended";

// An event rather than a reload: the signed-out `me` query gets a 401 too, and reloading on it
// would loop. AuthGate listens and swaps the page for the login form.
const sessionEndedExchange = mapExchange({
  onError(error) {
    if (error.response?.status === 401) {
      window.dispatchEvent(new Event(SESSION_ENDED_EVENT));
    }
  },
});

/** `fetch` defaults to the global one; tests pass a stub. */
export function createGraphqlClient(fetch?: typeof globalThis.fetch): Client {
  return new Client({
    url: "/graphql",
    fetch,
    exchanges: [cacheExchange, sessionEndedExchange, fetchExchange],
    // POST only: the session cookie is SameSite=Lax, which browsers also send on cross-site
    // top-level GET navigations, so the server never has to accept operations over GET.
    preferGetMethod: false,
  });
}

export const graphqlClient = createGraphqlClient();

/** The server's message for the first GraphQL error, else the transport error. */
export function errorMessage(error: CombinedError): string {
  return error.graphQLErrors[0]?.message ?? error.message;
}
