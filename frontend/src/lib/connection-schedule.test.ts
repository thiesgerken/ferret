import assert from "node:assert/strict";
import test from "node:test";
import { suggestedHourlySchedule } from "./connection-schedule";

const cases: { name: string; schedules: string[]; expected: string }[] = [
  { name: "no connections", schedules: [], expected: "0 * * * *" },
  { name: "one occupied minute on a circular clock", schedules: ["0 * * * *"], expected: "30 * * * *" },
  { name: "largest gap wraps through zero", schedules: ["0 * * * *", "10 * * * *"], expected: "35 * * * *" },
  { name: "equal gaps choose the lowest minute", schedules: ["0 * * * *", "30 * * * *"], expected: "15 * * * *" },
  { name: "lists and ranges occupy every matching minute", schedules: ["0-10,30-40 3 * * *"], expected: "20 * * * *" },
  { name: "wildcard steps", schedules: ["*/15 * * * *"], expected: "7 * * * *" },
  { name: "range steps", schedules: ["10-20/2 * * * *"], expected: "45 * * * *" },
  { name: "all minutes occupied", schedules: ["* * * * *"], expected: "0 * * * *" },
  { name: "cron aliases", schedules: ["@HOURLY", "30 * * * *"], expected: "15 * * * *" },
  { name: "optional seconds", schedules: ["15 10 * * * *"], expected: "40 * * * *" },
  { name: "optional year", schedules: ["0 20 3 * * * 2028"], expected: "50 * * * *" },
];

for (const { name, schedules, expected } of cases) {
  test(`suggests the most separated hourly minute: ${name}`, () => {
    assert.equal(suggestedHourlySchedule(schedules), expected);
  });
}
