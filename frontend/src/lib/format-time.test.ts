import assert from "node:assert/strict";
import test from "node:test";

import { formatDuration, timeAgo } from "./format-time";

test("durations show their two largest units", () => {
  assert.equal(formatDuration(0), "0 s");
  assert.equal(formatDuration(45), "45 s");
  assert.equal(formatDuration(80), "1 min 20 s");
  assert.equal(formatDuration(3_600), "1 h");
  assert.equal(formatDuration(7_505), "2 h 5 min");
  assert.equal(formatDuration(3_610), "1 h", "seconds beside hours are noise");
  assert.equal(formatDuration(273_600), "3 d 4 h");
});

test("times read relative to now", () => {
  const now = new Date("2026-10-17T12:00:00Z");
  assert.equal(timeAgo("2026-10-17T11:59:40Z", now), "just now");
  assert.equal(timeAgo("2026-10-17T11:57:00Z", now), "3 minutes ago");
  assert.equal(timeAgo("2026-10-17T09:00:00Z", now), "3 hours ago");
  assert.equal(timeAgo("2026-10-16T11:00:00Z", now), "yesterday");
  assert.equal(timeAgo("2026-10-17T12:30:00Z", now), "in 30 minutes");
});
