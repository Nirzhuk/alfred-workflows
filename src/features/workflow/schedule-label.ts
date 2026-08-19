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

const SIMPLE_FIELD = /^(?:\*|\*\/\d+|\d+(?:-\d+)?(?:,\d+(?:-\d+)?)*)$/;

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

/**
 * Sentence-style description of a six-field Alfred cron, or null when the
 * expression is not a known everyday pattern.
 */
export function describeSchedule(
  cron: string,
  nextRunAt?: string | null,
  timeZone?: string,
): string | null {
  const trimmed = cron.trim();
  if (!trimmed) return null;
  const compact = formatScheduleLabel(trimmed, nextRunAt, timeZone);
  if (compact === trimmed) return null;
  if (compact === "hourly") return "Every hour";
  const interval = compact.match(/^every (\d+) min$/);
  if (interval) return `Every ${interval[1]} minutes`;
  const daily = compact.match(/^(\d{2}:\d{2}) daily$/);
  if (daily) return `Every day at ${daily[1]}`;
  const weekdays = compact.match(/^(\d{2}:\d{2}) weekdays$/);
  if (weekdays) return `Weekdays at ${weekdays[1]}`;
  const named = compact.match(
    /^(\d{2}:\d{2}) (Sundays|Mondays|Tuesdays|Wednesdays|Thursdays|Fridays|Saturdays)$/,
  );
  if (named) return `${named[2]} at ${named[1]}`;
  return compact;
}

/** Locale date for a schedule fire time, or null when there is nothing to show. */
export function formatNextRunLabel(
  value: string | Date | null | undefined,
  timeZone?: string,
  locale?: string,
): string | null {
  if (value == null || value === "") return null;
  const date = value instanceof Date ? value : new Date(value);
  if (Number.isNaN(date.getTime())) return null;
  const day = date.toLocaleDateString(locale, {
    weekday: "long",
    day: "numeric",
    month: "short",
    year: "numeric",
    timeZone,
  });
  const time = date.toLocaleTimeString(locale, {
    hour: "2-digit",
    minute: "2-digit",
    hourCycle: "h23",
    timeZone,
  });
  return `${day} at ${time}`;
}

function fieldMatches(raw: string, value: number, sundayWrap = false): boolean {
  if (raw === "*") return true;
  const step = raw.match(/^\*\/(\d+)$/);
  if (step) {
    const n = Number(step[1]);
    return n > 0 && value % n === 0;
  }
  const allowed = new Set<number>();
  for (const part of raw.split(",")) {
    const range = part.match(/^(\d+)-(\d+)$/);
    if (range) {
      const from = Number(range[1]);
      const to = Number(range[2]);
      if (from > to) return false;
      for (let i = from; i <= to; i++) {
        allowed.add(sundayWrap && i === 7 ? 0 : i);
      }
      continue;
    }
    if (!/^\d+$/.test(part)) return false;
    const n = Number(part);
    allowed.add(sundayWrap && n === 7 ? 0 : n);
  }
  return allowed.has(value);
}

/**
 * Next fire time for a simple six-field Alfred cron, in UTC like the desktop
 * scheduler. Returns null for names, aliases, or other unsupported syntax.
 */
export function previewNextRunAt(
  cron: string,
  after: Date = new Date(),
): Date | null {
  const fields = cron.trim().split(/\s+/);
  if (fields.length !== 6 || fields.some((field) => !SIMPLE_FIELD.test(field))) {
    return null;
  }

  const [seconds, minutes, hours, dayOfMonth, month, dayOfWeek] = fields;
  const cursor = new Date(after.getTime());
  cursor.setUTCMilliseconds(0);
  cursor.setUTCSeconds(cursor.getUTCSeconds() + 1);

  const exactSecond = /^\d+$/.test(seconds) ? Number(seconds) : null;
  if (exactSecond != null) {
    if (cursor.getUTCSeconds() > exactSecond) {
      cursor.setUTCMinutes(cursor.getUTCMinutes() + 1);
    }
    cursor.setUTCSeconds(exactSecond);
  }

  const limit = after.getTime() + 366 * 24 * 60 * 60 * 1000;
  for (let i = 0; i < 366 * 24 * 60 && cursor.getTime() <= limit; i++) {
    const domStar = dayOfMonth === "*";
    const dowStar = dayOfWeek === "*";
    const domOk = fieldMatches(dayOfMonth, cursor.getUTCDate());
    const dowOk = fieldMatches(dayOfWeek, cursor.getUTCDay(), true);
    const dayOk = domStar || dowStar ? domOk && dowOk : domOk || dowOk;

    if (
      fieldMatches(seconds, cursor.getUTCSeconds()) &&
      fieldMatches(minutes, cursor.getUTCMinutes()) &&
      fieldMatches(hours, cursor.getUTCHours()) &&
      fieldMatches(month, cursor.getUTCMonth() + 1) &&
      dayOk
    ) {
      return new Date(cursor.getTime());
    }

    cursor.setUTCMinutes(cursor.getUTCMinutes() + 1);
    if (exactSecond != null) cursor.setUTCSeconds(exactSecond);
    else cursor.setUTCSeconds(0);
  }

  return null;
}
