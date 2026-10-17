import assert from "node:assert/strict";
import test from "node:test";

import { formatDuration, timeAgo } from "./format-time";

test("durations show their two largest units", () => {
  assert.equal(formatDuration(0, "en"), "0 sec");
  assert.equal(formatDuration(45, "en"), "45 sec");
  assert.equal(formatDuration(80, "en"), "1 min, 20 sec");
  assert.equal(formatDuration(3_600, "en"), "1 hr");
  assert.equal(formatDuration(7_505, "en"), "2 hr, 5 min");
  assert.equal(formatDuration(3_610, "en"), "1 hr", "seconds beside hours are noise");
  assert.equal(formatDuration(273_600, "en"), "3 days, 4 hr");
  assert.equal(formatDuration(7_505, "de"), "2 Std., 5 Min.");
});

test("times read relative to now", () => {
  const now = new Date("2026-10-17T12:00:00Z");
  assert.equal(timeAgo("2026-10-17T11:59:40Z", now, "en"), "now");
  assert.equal(timeAgo("2026-10-17T11:57:00Z", now, "en"), "3 minutes ago");
  assert.equal(timeAgo("2026-10-17T09:00:00Z", now, "en"), "3 hours ago");
  assert.equal(timeAgo("2026-10-16T11:00:00Z", now, "en"), "yesterday");
  assert.equal(timeAgo("2026-10-17T12:30:00Z", now, "en"), "in 30 minutes");
  assert.equal(timeAgo("2026-10-17T11:57:00Z", now, "de"), "vor 3 Minuten");
});
