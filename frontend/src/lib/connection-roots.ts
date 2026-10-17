import { useMemo } from "react";
import { useQuery } from "urql";
import { graphql } from "@/gql";

// A connection's root, so a source reads as the connection's name and the path below it.
const ConnectionRootsQuery = graphql(`
  query ConnectionRoots {
    connections {
      id
      source
    }
  }
`);

/** Connection ids to their source roots, for `sourceLabel`. Undefined until loaded, so a
 * connection's sources don't flash its name over the full path. */
export function useConnectionRoots(): Map<string, string> | undefined {
  const [{ data }] = useQuery({ query: ConnectionRootsQuery });
  return useMemo(() => data && new Map(data.connections.map((connection) => [connection.id, connection.source])), [data]);
}
