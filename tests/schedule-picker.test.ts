import { describe, expect, test } from "bun:test";
import {
  cronToPicker,
  DEFAULT_SCHEDULE_PICKER,
  parseTimeInput,
  pickerToCron,
  timeInputValue,
} from "../src/features/workflow/schedule-picker";

const noonUtc = new Date("2026-08-19T12:00:00.000Z");

describe("pickerToCron", () => {
  test("keeps interval schedules independent of the clock", () => {
    expect(pickerToCron({ ...DEFAULT_SCHEDULE_PICKER, repeat: "hourly" }, "UTC")).toBe(
      "0 0 * * * *",
    );
    expect(
      pickerToCron({ ...DEFAULT_SCHEDULE_PICKER, repeat: "every_15m" }, "UTC"),
    ).toBe("0 */15 * * * *");
  });

  test("encodes a local daily time as UTC cron", () => {
    expect(
      pickerToCron(
        { repeat: "daily", hour: 9, minute: 0, days: [1] },
        "UTC",
        noonUtc,
      ),
    ).toBe("0 0 9 * * *");
    expect(
      pickerToCron(
        { repeat: "daily", hour: 11, minute: 0, days: [1] },
        "Europe/Madrid",
        noonUtc,
      ),
    ).toBe("0 0 9 * * *");
  });

  test("shifts weekday when local time crosses midnight UTC", () => {
    expect(
      pickerToCron(
        { repeat: "weekly", hour: 1, minute: 0, days: [1] },
        "Europe/Madrid",
        noonUtc,
      ),
    ).toBe("0 0 23 * * 0");
  });

  test("encodes weekdays and selected days", () => {
    expect(
      pickerToCron(
        { repeat: "weekdays", hour: 9, minute: 30, days: [1] },
        "UTC",
        noonUtc,
      ),
    ).toBe("0 30 9 * * 1-5");
    expect(
      pickerToCron(
        { repeat: "weekly", hour: 14, minute: 15, days: [1, 3, 5] },
        "UTC",
        noonUtc,
      ),
    ).toBe("0 15 14 * * 1,3,5");
  });
});

describe("cronToPicker", () => {
  test("reads interval and daily schedules", () => {
    expect(cronToPicker("0 0 * * * *")).toEqual({
      ...DEFAULT_SCHEDULE_PICKER,
      repeat: "hourly",
    });
    expect(cronToPicker("0 */15 * * * *")).toEqual({
      ...DEFAULT_SCHEDULE_PICKER,
      repeat: "every_15m",
    });
    expect(cronToPicker("0 0 9 * * *", "UTC", noonUtc)).toEqual({
      repeat: "daily",
      hour: 9,
      minute: 0,
      days: [1],
    });
    expect(cronToPicker("0 0 9 * * *", "Europe/Madrid", noonUtc)).toEqual({
      repeat: "daily",
      hour: 11,
      minute: 0,
      days: [1],
    });
  });

  test("round-trips a Monday morning after a timezone shift", () => {
    const cron = pickerToCron(
      { repeat: "weekly", hour: 1, minute: 0, days: [1] },
      "Europe/Madrid",
      noonUtc,
    );
    expect(cronToPicker(cron, "Europe/Madrid", noonUtc)).toEqual({
      repeat: "weekly",
      hour: 1,
      minute: 0,
      days: [1],
    });
  });

  test("opens unsupported expressions in cron mode", () => {
    expect(cronToPicker("0 0 9 1 * *")).toBeNull();
    expect(cronToPicker("0 0 9 * JAN 1")).toBeNull();
  });
});

describe("time input helpers", () => {
  test("round-trips HH:MM values", () => {
    expect(timeInputValue(9, 0)).toBe("09:00");
    expect(parseTimeInput("09:00")).toEqual({ hour: 9, minute: 0 });
    expect(parseTimeInput("14:30")).toEqual({ hour: 14, minute: 30 });
    expect(parseTimeInput("")).toBeNull();
  });
});
