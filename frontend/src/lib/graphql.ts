import { Client, cacheExchange, fetchExchange } from "urql";

export const graphqlClient = new Client({
  url: "/graphql",
  exchanges: [cacheExchange, fetchExchange],
});
