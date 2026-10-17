// Preloaded via `--import`: react-dom checks for a DOM once, when it is first imported, and falls
// back to legacy event polyfills if there is none. So the globals must exist before any test
// module loads. node --test runs each test file in its own process, so tests don't share a DOM.
import { JSDOM } from "jsdom";

const dom = new JSDOM("<!doctype html><html><body></body></html>", {
  url: "https://ferret.example.test/",
});

const globals = {
  window: dom.window,
  self: dom.window,
  document: dom.window.document,
  navigator: dom.window.navigator,
  HTMLElement: dom.window.HTMLElement,
  Node: dom.window.Node,
  Event: dom.window.Event,
  MouseEvent: dom.window.MouseEvent,
  // Node's own FormData can't read a jsdom form.
  FormData: dom.window.FormData,
};
for (const [key, value] of Object.entries(globals)) {
  Object.defineProperty(globalThis, key, { configurable: true, writable: true, value });
}
Object.defineProperty(globalThis, "IS_REACT_ACT_ENVIRONMENT", { configurable: true, writable: true, value: true });
