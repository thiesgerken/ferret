"use client";

import type { ReactNode } from "react";
import { Provider as GraphqlProvider } from "urql";
import { ThemeProvider } from "@/components/theme-provider";
import { graphqlClient } from "@/lib/graphql";

export function AppProviders({ children }: { children: ReactNode }) {
  return (
    <ThemeProvider>
      <GraphqlProvider value={graphqlClient}>{children}</GraphqlProvider>
    </ThemeProvider>
  );
}
