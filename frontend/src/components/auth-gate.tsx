"use client";

import type { ReactNode } from "react";
import { useQuery } from "urql";
import { AppMenu } from "@/components/app-menu";
import { LoginForm } from "@/components/login-form";
import { graphql } from "@/gql";

const MeQuery = graphql(`
  query Me {
    me {
      username
      displayName
      isAdmin
    }
  }
`);

/** Shows the login form in place of any page while signed out, so deep links survive a sign-in. */
export function AuthGate({ children }: { children: ReactNode }) {
  const [{ data, error }] = useQuery({ query: MeQuery });

  if (error?.response?.status === 401) {
    // A full reload drops whatever the previous state held and refetches `me` with the new cookie.
    return <LoginForm onSignedIn={() => window.location.reload()} />;
  }
  if (error) {
    return <p className="p-8 text-center text-sm text-destructive">Couldn&apos;t reach the server: {error.message}</p>;
  }
  if (!data) {
    return null;
  }

  return (
    <>
      <header className="fixed top-0 right-0 p-4">
        <AppMenu user={data.me} />
      </header>
      {children}
    </>
  );
}
