"use client";

import { useTranslations } from "next-intl";
import { useEffect, useState, type ReactNode } from "react";
import { useQuery } from "urql";
import { AppMenu } from "@/components/app-menu";
import { LoginForm } from "@/components/login-form";
import { graphql } from "@/gql";
import { SESSION_ENDED_EVENT } from "@/lib/graphql";

const MeQuery = graphql(`
  query Me {
    version
    me {
      ...AppMenuUser
    }
  }
`);

/** Shows the login form in place of any page while signed out, so deep links survive a sign-in. */
export function AuthGate({ children }: { children: ReactNode }) {
  const t = useTranslations("app");
  const [{ data, error }] = useQuery({ query: MeQuery });
  const [sessionEnded, setSessionEnded] = useState(false);

  useEffect(() => {
    const end = () => setSessionEnded(true);
    window.addEventListener(SESSION_ENDED_EVENT, end);
    return () => window.removeEventListener(SESSION_ENDED_EVENT, end);
  }, []);

  // The 401 of `me` itself is checked directly too, so its error never flashes up before the event.
  if (sessionEnded || error?.response?.status === 401) {
    // A full reload drops the previous session's cached data and refetches `me` with the new cookie.
    return <LoginForm onSignedIn={() => window.location.reload()} />;
  }
  if (error) {
    return <p className="p-8 text-center text-sm text-destructive">{t("failed", { message: error.message })}</p>;
  }
  if (!data) {
    return null;
  }

  return (
    <>
      <header className="fixed top-0 right-0 p-4">
        <AppMenu user={data.me} serverVersion={data.version} />
      </header>
      {children}
    </>
  );
}
