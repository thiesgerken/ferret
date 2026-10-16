import type { CodegenConfig } from "@graphql-codegen/cli";

const config: CodegenConfig = {
  schema: "schema.graphql",
  documents: ["src/**/*.{ts,tsx}", "!src/gql/**"],
  // Until the first query exists there are no documents, which codegen treats as an error.
  ignoreNoDocuments: true,
  generates: {
    "src/gql/": {
      preset: "client",
    },
  },
};

export default config;
