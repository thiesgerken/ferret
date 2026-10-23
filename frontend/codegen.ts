import type { CodegenConfig } from "@graphql-codegen/cli";

const config: CodegenConfig = {
  schema: "schema.graphql",
  documents: ["src/**/*.{ts,tsx}", "!src/gql/**"],
  generates: {
    "src/gql/": {
      preset: "client",
      config: {
        // String unions instead of TypeScript enums, so code and tests can write "SEARCH_READ".
        enumsAsTypes: true,
        // RFC 3339 strings on the wire.
        scalars: { DateTime: "string" },
      },
    },
  },
};

export default config;
