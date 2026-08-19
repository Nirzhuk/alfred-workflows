export type ScheduleRepeat =
  | "hourly"
  | "every_15m"
  | "daily"
  | "weekdays"
  | "weekly";

export type SchedulePickerValue = {
  repeat: ScheduleRepeat;
  /** Local wall-clock hour, 0-23. */
  hour: number;
  /** Local wall-clock minute, 0-59. */
  minute: number;
  /** Local JS weekdays, 0 = Sunday. Used when repeat is weekly. */
  days: number[];
};

export const DEFAULT_SCHEDULE_PICKER: SchedulePickerValue = {
  repeat: "daily",
  hour: 9,
  minute: 0,
  days: [1],
};

export const SCHEDULE_REPEAT_OPTIONS: Array<{
  id: ScheduleRepeat;
  label: string;
}> = [
  { id: "hourly", label: "Every hour" },
  { id: "every_15m", label: "Every 15 minutes" },
  { id: "daily", label: "Every day" },
  { id: "weekdays", label: "Weekdays" },
  { id: "weekly", label: "On selected days" },
];

export const SCHEDULE_WEEKDAYS: Array<{
  value: number;
  label: string;
  name: string;
}> = [
  { value: 1, label: "Mon", name: "Monday" },
  { value: 2, label: "Tue", name: "Tuesday" },
  { value: 3, label: "Wed", name: "Wednesday" },
  { value: 4, label: "Thu", name: "Thursday" },
  { value: 5, label: "Fri", name: "Friday" },
  { value: 6, label: "Sat", name: "Saturday" },
  { value: 0, label: "Sun", name: "Sunday" },
];

const WEEKDAY_SHORT = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
const WEEKDAYS = [1, 2, 3, 4, 5];

type ZoneParts = {
  year: number;
  month: number;
  day: number;
  hour: number;
  minute: number;
  second: number;
  weekday: number;
};

function weekdayIndex(short: string): number {
  const index = WEEKDAY_SHORT.indexOf(short);
  return index === -1 ? 0 : index;
}

function partsInZone(date: Date, timeZone?: string): ZoneParts {
  if (!timeZone) {
    return {
      year: date.getFullYear(),
      month: date.getMonth() + 1,
      day: date.getDate(),
      hour: date.getHours(),
      minute: date.getMinutes(),
      second: date.getSeconds(),
      weekday: date.getDay(),
    };
  }

  const formatter = new Intl.DateTimeFormat("en-US", {
    timeZone,
    hourCycle: "h23",
    year: "numeric",
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
    weekday: "short",
  });
  const map: Record<string, string> = {};
  for (const part of formatter.formatToParts(date)) {
    if (part.type !== "literal") map[part.type] = part.value;
  }
  return {
    year: Number(map.year),
    month: Number(map.month),
    day: Number(map.day),
    hour: Number(map.hour),
    minute: Number(map.minute),
    second: Number(map.second),
    weekday: weekdayIndex(map.weekday),
  };
}

function zoneOffsetMs(date: Date, timeZone?: string): number {
  if (!timeZone) return -date.getTimezoneOffset() * 60_000;
  const parts = partsInZone(date, timeZone);
  return (
    Date.UTC(
      parts.year,
      parts.month - 1,
      parts.day,
      parts.hour,
      parts.minute,
      parts.second,
    ) - date.getTime()
  );
}

function wallClockToDate(
  hour: number,
  minute: number,
  timeZone: string | undefined,
  ref: Date,
): Date {
  const parts = partsInZone(ref, timeZone);
  const wallAsUtc = Date.UTC(
    parts.year,
    parts.month - 1,
    parts.day,
    hour,
    minute,
    0,
  );
  const guessed = wallAsUtc - zoneOffsetMs(ref, timeZone);
  return new Date(wallAsUtc - zoneOffsetMs(new Date(guessed), timeZone));
}

function uniqueSorted(days: number[]): number[] {
  return [...new Set(days.map((day) => ((day % 7) + 7) % 7))].sort(
    (a, b) => a - b,
  );
}

function sameDays(left: number[], right: number[]): boolean {
  const a = uniqueSorted(left);
  const b = uniqueSorted(right);
  return a.length === b.length && a.every((day, index) => day === b[index]);
}

function encodeDow(days: number[]): string {
  const unique = uniqueSorted(days);
  if (unique.length === 7) return "*";
  if (sameDays(unique, WEEKDAYS)) return "1-5";
  return unique.join(",");
}

function parseDow(raw: string): number[] | "any" | null {
  if (raw === "*") return "any";
  const days: number[] = [];
  for (const part of raw.split(",")) {
    const range = part.match(/^(\d+)-(\d+)$/);
    if (range) {
      const from = Number(range[1]);
      const to = Number(range[2]);
      if (from > to || from < 0 || to > 7) return null;
      for (let day = from; day <= to; day++) {
        days.push(day === 7 ? 0 : day);
      }
      continue;
    }
    if (!/^\d+$/.test(part)) return null;
    const day = Number(part);
    if (day < 0 || day > 7) return null;
    days.push(day === 7 ? 0 : day);
  }
  return uniqueSorted(days);
}

function shiftDays(days: number[], shift: number): number[] {
  return uniqueSorted(days.map((day) => day + shift));
}

function needsTime(repeat: ScheduleRepeat): boolean {
  return repeat === "daily" || repeat === "weekdays" || repeat === "weekly";
}

export function pickerUsesTime(repeat: ScheduleRepeat): boolean {
  return needsTime(repeat);
}

export function pickerToCron(
  picker: SchedulePickerValue,
  timeZone?: string,
  now: Date = new Date(),
): string {
  if (picker.repeat === "hourly") return "0 0 * * * *";
  if (picker.repeat === "every_15m") return "0 */15 * * * *";

  const hour = Math.min(23, Math.max(0, Math.trunc(picker.hour)));
  const minute = Math.min(59, Math.max(0, Math.trunc(picker.minute)));
  const fire = wallClockToDate(hour, minute, timeZone, now);
  const utcHour = fire.getUTCHours();
  const utcMinute = fire.getUTCMinutes();
  const localDay = partsInZone(fire, timeZone).weekday;
  const shift = fire.getUTCDay() - localDay;

  const localDays =
    picker.repeat === "weekdays"
      ? WEEKDAYS
      : picker.repeat === "weekly"
        ? picker.days.length > 0
          ? picker.days
          : [1]
        : [];

  if (picker.repeat === "daily") {
    return `0 ${utcMinute} ${utcHour} * * *`;
  }

  return `0 ${utcMinute} ${utcHour} * * ${encodeDow(shiftDays(localDays, shift))}`;
}

export function cronToPicker(
  cron: string,
  timeZone?: string,
  now: Date = new Date(),
): SchedulePickerValue | null {
  const fields = cron.trim().split(/\s+/);
  if (fields.length !== 6) return null;
  const [seconds, minutes, hours, dayOfMonth, month, dayOfWeek] = fields;
  if (seconds !== "0" && seconds !== "00") return null;
  if (dayOfMonth !== "*" || month !== "*") return null;

  if (hours === "*" && minutes === "0" && dayOfWeek === "*") {
    return { ...DEFAULT_SCHEDULE_PICKER, repeat: "hourly" };
  }
  if (hours === "*" && minutes === "*/15" && dayOfWeek === "*") {
    return { ...DEFAULT_SCHEDULE_PICKER, repeat: "every_15m" };
  }
  if (!/^\d{1,2}$/.test(hours) || !/^\d{1,2}$/.test(minutes)) return null;

  const utcHour = Number(hours);
  const utcMinute = Number(minutes);
  if (utcHour > 23 || utcMinute > 59) return null;

  const utcDays = parseDow(dayOfWeek);
  if (utcDays == null) return null;

  const utcInstant = new Date(
    Date.UTC(
      now.getUTCFullYear(),
      now.getUTCMonth(),
      now.getUTCDate(),
      utcHour,
      utcMinute,
      0,
      0,
    ),
  );
  const local = partsInZone(utcInstant, timeZone);
  const shift = local.weekday - utcInstant.getUTCDay();

  if (utcDays === "any") {
    return {
      repeat: "daily",
      hour: local.hour,
      minute: local.minute,
      days: [1],
    };
  }

  const localDays = shiftDays(utcDays, shift);
  if (sameDays(localDays, WEEKDAYS)) {
    return {
      repeat: "weekdays",
      hour: local.hour,
      minute: local.minute,
      days: [1],
    };
  }

  return {
    repeat: "weekly",
    hour: local.hour,
    minute: local.minute,
    days: localDays.length > 0 ? localDays : [1],
  };
}

export function timeInputValue(hour: number, minute: number): string {
  return `${String(hour).padStart(2, "0")}:${String(minute).padStart(2, "0")}`;
}

export function parseTimeInput(value: string): { hour: number; minute: number } | null {
  const match = value.trim().match(/^(\d{1,2}):(\d{2})/);
  if (!match) return null;
  const hour = Number(match[1]);
  const minute = Number(match[2]);
  if (hour > 23 || minute > 59) return null;
  return { hour, minute };
}
