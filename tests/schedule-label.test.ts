import { describe, expect, test } from "bun:test";
import {
  describeSchedule,
  formatNextRunLabel,
  formatScheduleLabel,
  previewNextRunAt,
} from "../src/features/workflow/schedule-label";

describe("formatScheduleLabel", () => {
  test("formats daily and weekday schedules", () => {
    expect(formatScheduleLabel("0 0 9 * * *")).toBe("09:00 daily");
    expect(formatScheduleLabel("0 0 9 * * 1-5")).toBe("09:00 weekdays");
    expect(formatScheduleLabel("0 30 14 * * 1")).toBe("14:30 Mondays");
  });

  test("formats recurring interval schedules", () => {
    expect(formatScheduleLabel("0 0 * * * *")).toBe("hourly");
    expect(formatScheduleLabel("0 */15 * * * *")).toBe("every 15 min");
  });

  test("uses the actual local next-run time instead of the UTC cron hour", () => {
    expect(
      formatScheduleLabel(
        "0 0 9 * * 1-5",
        "2026-08-12T09:00:00Z",
        "Europe/Madrid",
      ),
    ).toBe("11:00 weekdays");
  });

  test("keeps unsupported custom expressions accurate", () => {
    expect(formatScheduleLabel("0 0 9 1 * *")).toBe("0 0 9 1 * *");
  });
});

describe("describeSchedule", () => {
  test("turns known patterns into everyday language", () => {
    expect(describeSchedule("0 0 * * * *")).toBe("Every hour");
    expect(describeSchedule("0 */15 * * * *")).toBe("Every 15 minutes");
    expect(describeSchedule("0 0 9 * * *")).toBe("Every day at 09:00");
    expect(describeSchedule("0 0 9 * * 1-5")).toBe("Weekdays at 09:00");
    expect(describeSchedule("0 0 9 * * 1")).toBe("Mondays at 09:00");
  });

  test("returns null for empty or unsupported expressions", () => {
    expect(describeSchedule("")).toBeNull();
    expect(describeSchedule("0 0 9 1 * *")).toBeNull();
  });
});

describe("formatNextRunLabel", () => {
  test("formats a fire time in plain language", () => {
    expect(
      formatNextRunLabel("2026-08-24T09:00:00.000Z", "UTC", "en-GB"),
    ).toMatch(/^Monday,? 24 Aug 2026 at 09:00$/);
  });

  test("hides missing or invalid times", () => {
    expect(formatNextRunLabel(null)).toBeNull();
    expect(formatNextRunLabel("")).toBeNull();
    expect(formatNextRunLabel("not-a-date")).toBeNull();
  });
});

describe("previewNextRunAt", () => {
  test("finds the next hourly and interval fire times", () => {
    expect(
      previewNextRunAt("0 0 * * * *", new Date("2026-08-19T09:00:00.000Z"))?.toISOString(),
    ).toBe("2026-08-19T10:00:00.000Z");
    expect(
      previewNextRunAt("0 */15 * * * *", new Date("2026-08-19T09:07:00.000Z"))?.toISOString(),
    ).toBe("2026-08-19T09:15:00.000Z");
  });

  test("finds the next weekday and Monday morning", () => {
    expect(
      previewNextRunAt("0 0 9 * * *", new Date("2026-08-19T09:00:00.000Z"))?.toISOString(),
    ).toBe("2026-08-20T09:00:00.000Z");
    expect(
      previewNextRunAt("0 0 9 * * 1-5", new Date("2026-08-21T09:00:00.000Z"))?.toISOString(),
    ).toBe("2026-08-24T09:00:00.000Z");
    expect(
      previewNextRunAt("0 0 9 * * 1", new Date("2026-08-19T10:00:00.000Z"))?.toISOString(),
    ).toBe("2026-08-24T09:00:00.000Z");
  });

  test("returns null for unsupported syntax", () => {
    expect(previewNextRunAt("0 0 9 1 JAN *")).toBeNull();
    expect(previewNextRunAt("not cron")).toBeNull();
  });
});
