import { NextIntlClientProvider } from "next-intl";
import type { ReactNode } from "react";
import { act } from "react";
import { createRoot } from "react-dom/client";
import { MESSAGES } from "@/lib/i18n";

type RenderHandle = {
  container: HTMLDivElement;
  unmount: () => Promise<void>;
};

/** Renders in English, in UTC so dates read the same on every machine. */
export async function renderReact(node: ReactNode): Promise<RenderHandle> {
  const container = document.createElement("div");
  document.body.appendChild(container);
  const root = createRoot(container);

  await act(async () => {
    root.render(
      <NextIntlClientProvider locale="en" messages={MESSAGES.en} timeZone="UTC">
        {node}
      </NextIntlClientProvider>,
    );
  });

  return {
    container,
    async unmount(): Promise<void> {
      await act(async () => {
        root.unmount();
      });
      container.remove();
    },
  };
}

/** Lets pending fetches and the state updates they trigger settle. */
export async function settle(): Promise<void> {
  await act(async () => {
    await new Promise((resolve) => setTimeout(resolve, 0));
  });
}
