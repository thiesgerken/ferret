"use client";

import { ThemeProvider } from "next-themes";
import type { ReactNode } from "react";
import { Provider as GraphqlProvider } from "urql";
import { graphqlClient } from "@/lib/graphql";

export function AppProviders({ children }: { children: ReactNode }) {
  return (
    <ThemeProvider attribute="class" defaultTheme="system" enableSystem>
      <GraphqlProvider value={graphqlClient}>{children}</GraphqlProvider>
    </ThemeProvider>
  );
}
