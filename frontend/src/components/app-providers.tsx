"use client";

import { ThemeProvider } from "next-themes";
import type { ReactNode } from "react";
import { Provider as GraphqlProvider } from "urql";
import { AuthGate } from "@/components/auth-gate";
import { LocaleProvider } from "@/components/locale-provider";
import { graphqlClient } from "@/lib/graphql";

export function AppProviders({ children }: { children: ReactNode }) {
  return (
    <ThemeProvider attribute="class" defaultTheme="system" enableSystem>
      <LocaleProvider>
        <GraphqlProvider value={graphqlClient}>
          <AuthGate>{children}</AuthGate>
        </GraphqlProvider>
      </LocaleProvider>
    </ThemeProvider>
  );
}
