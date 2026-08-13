const WEEKDAYS: Record<string, string> = {
  "0": "Sundays",
  "1": "Mondays",
  "2": "Tuesdays",
  "3": "Wednesdays",
  "4": "Thursdays",
  "5": "Fridays",
  "6": "Saturdays",
  "7": "Sundays",
};

function clockTime(hour: string, minute: string): string | null {
  if (!/^\d{1,2}$/.test(hour) || !/^\d{1,2}$/.test(minute)) return null;
  const hours = Number(hour);
  const minutes = Number(minute);
  if (hours > 23 || minutes > 59) return null;
  return `${String(hours).padStart(2, "0")}:${String(minutes).padStart(2, "0")}`;
}

function localClockTime(
  nextRunAt: string | null | undefined,
  timeZone?: string,
): string | null {
  if (!nextRunAt) return null;
  const date = new Date(nextRunAt);
  if (Number.isNaN(date.getTime())) return null;
  const parts = new Intl.DateTimeFormat(undefined, {
    hour: "2-digit",
    minute: "2-digit",
    hourCycle: "h23",
    timeZone,
  }).formatToParts(date);
  const hour = parts.find((part) => part.type === "hour")?.value;
  const minute = parts.find((part) => part.type === "minute")?.value;
  return hour && minute ? `${hour}:${minute}` : null;
}

/** Compact, human-readable text for a six-field Alfred cron expression. */
export function formatScheduleLabel(
  cron: string,
  nextRunAt?: string | null,
  timeZone?: string,
): string {
  const fields = cron.trim().split(/\s+/);
  if (fields.length !== 6) return cron;

  const [, minute, hour, dayOfMonth, month, dayOfWeek] = fields;
  if (dayOfMonth === "*" && month === "*" && dayOfWeek === "*") {
    if (hour === "*" && minute === "0") return "hourly";
    const interval = minute.match(/^\*\/(\d+)$/)?.[1];
    if (hour === "*" && interval) return `every ${interval} min`;
  }

  const time = localClockTime(nextRunAt, timeZone) ?? clockTime(hour, minute);
  if (!time || dayOfMonth !== "*" || month !== "*") return cron;
  if (dayOfWeek === "*") return `${time} daily`;
  if (dayOfWeek === "1-5") return `${time} weekdays`;
  if (WEEKDAYS[dayOfWeek]) return `${time} ${WEEKDAYS[dayOfWeek]}`;
  return cron;
}
