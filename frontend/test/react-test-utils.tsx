import type { ReactNode } from "react";
import { act } from "react";
import { createRoot } from "react-dom/client";

type RenderHandle = {
  container: HTMLDivElement;
  unmount: () => Promise<void>;
};

export async function renderReact(node: ReactNode): Promise<RenderHandle> {
  const container = document.createElement("div");
  document.body.appendChild(container);
  const root = createRoot(container);

  await act(async () => {
    root.render(node);
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
