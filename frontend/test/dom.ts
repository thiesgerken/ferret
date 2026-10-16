// Preloaded via `--import`: react-dom checks for a DOM once, when it is first imported, and falls
// back to legacy event polyfills if there is none. So the globals must exist before any test
// module loads. node --test runs each test file in its own process, so tests don't share a DOM.
import { JSDOM } from "jsdom";

const dom = new JSDOM("<!doctype html><html><body></body></html>", {
  url: "https://ferret.example.test/",
});

for (const key of ["window", "document", "navigator", "HTMLElement", "Node", "Event", "MouseEvent"] as const) {
  Object.defineProperty(globalThis, key, {
    configurable: true,
    writable: true,
    value: key === "window" ? dom.window : dom.window[key],
  });
}
Object.defineProperty(globalThis, "IS_REACT_ACT_ENVIRONMENT", { configurable: true, writable: true, value: true });
