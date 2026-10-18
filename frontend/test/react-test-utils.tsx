import { afterEach } from "node:test";
import { NextIntlClientProvider } from "next-intl";
import type { ReactNode } from "react";
import { act } from "react";
import { createRoot } from "react-dom/client";
import { MESSAGES } from "@/lib/i18n";

type RenderHandle = {
  container: HTMLDivElement;
  unmount: () => Promise<void>;
};

const mounted = new Set<RenderHandle>();

// A failed assertion skips a test's own unmount, and the timers of a still mounted component (a
// clock, polling) would then keep the run from ever ending.
afterEach(async () => {
  for (const handle of mounted) {
    await handle.unmount();
  }
});

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

  const handle: RenderHandle = {
    container,
    async unmount(): Promise<void> {
      if (!mounted.delete(handle)) {
        return;
      }
      await act(async () => {
        root.unmount();
      });
      container.remove();
    },
  };
  mounted.add(handle);
  return handle;
}

/** Lets pending fetches and the state updates they trigger settle. */
export async function settle(): Promise<void> {
  await act(async () => {
    await new Promise((resolve) => setTimeout(resolve, 0));
  });
}
