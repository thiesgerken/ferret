const MINUTES_PER_HOUR = 60;

/** Consider minute occupancy, regardless of the hour or day on which a connection runs. */
export function suggestedHourlySchedule(schedules: string[]): string {
  const occupied = new Set(schedules.flatMap(scheduleMinutes));
  let bestMinute = 0;
  let bestDistance = -1;
  for (let minute = 0; minute < MINUTES_PER_HOUR; minute++) {
    let distance = MINUTES_PER_HOUR;
    for (const used of occupied) {
      const difference = Math.abs(minute - used);
      distance = Math.min(distance, difference, MINUTES_PER_HOUR - difference);
    }
    // Strict comparison keeps the lowest minute when several gaps are equally good.
    if (distance > bestDistance) {
      bestMinute = minute;
      bestDistance = distance;
    }
  }
  return `${bestMinute} * * * *`;
}

function scheduleMinutes(schedule: string): number[] {
  // Croner accepts these aliases and optional seconds/year in addition to five-field cron.
  if (/^@(annually|yearly|monthly|weekly|daily|hourly)$/i.test(schedule.trim())) {
    return [0];
  }
  const fields = schedule.trim().split(/\s+/);
  if (fields.length < 5 || fields.length > 7) {
    throw new Error(`Invalid connection schedule: ${schedule}`);
  }
  const minuteField = fields[fields.length === 5 ? 0 : 1];
  return minuteField.split(",").flatMap((part) => {
    const match = /^(\*|\d+(?:-\d+)?)(?:\/(\d+))?$/.exec(part);
    if (!match) {
      throw new Error(`Invalid cron minute field: ${minuteField}`);
    }
    const [, range, stride] = match;
    const [start, end] = range === "*" ? [0, 59] : range.split("-").map(Number);
    const last = end ?? start;
    const step = stride === undefined ? 1 : Number(stride);
    if (start < 0 || last >= MINUTES_PER_HOUR || start > last || step < 1) {
      throw new Error(`Invalid cron minute field: ${minuteField}`);
    }
    const minutes: number[] = [];
    for (let minute = start; minute <= last; minute += step) {
      minutes.push(minute);
    }
    return minutes;
  });
}
