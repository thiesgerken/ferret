import assert from "node:assert/strict";
import test from "node:test";

import { exploreHref, monthRange, parseExplore, type ExploreState } from "./explore-url";

const DEFAULT: ExploreState = {
  sources: [],
  contentTypes: [],
  authors: [],
  month: null,
  orderBy: "CREATED_AT",
  descending: true,
  page: 0,
};

test("the default state is the bare route", () => {
  assert.equal(exploreHref(DEFAULT), "/explore");
  assert.deepEqual(parseExplore(new URLSearchParams()), DEFAULT);
});

test("a state survives the round trip through the URL", () => {
  const state: ExploreState = {
    sources: ["mail/personal", "notes"],
    contentTypes: ["EMAIL"],
    authors: ["Ada"],
    month: "2026-01",
    orderBy: "TITLE",
    descending: false,
    page: 2,
  };

  const href = exploreHref(state);

  assert.deepEqual(parseExplore(new URLSearchParams(href.split("?")[1])), state);
});

test("unknown values fall back to the defaults", () => {
  const params = new URLSearchParams("type=PDF&month=January&sort=SIZE&page=-1");

  assert.deepEqual(parseExplore(params), DEFAULT);
});

test("a month spans its first instant to the next month's, in UTC", () => {
  assert.deepEqual(monthRange("2026-12"), {
    from: "2026-12-01T00:00:00.000Z",
    to: "2027-01-01T00:00:00.000Z",
  });
});
