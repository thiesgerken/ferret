import { Client, cacheExchange, fetchExchange } from "urql";

export const graphqlClient = new Client({
  url: "/graphql",
  exchanges: [cacheExchange, fetchExchange],
  // POST only: the session cookie is SameSite=Lax, which browsers also send on cross-site top-level
  // GET navigations, so the server never has to accept operations over GET.
  preferGetMethod: false,
});
