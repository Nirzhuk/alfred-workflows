import { describe, expect, test } from "bun:test";
import { formatScheduleLabel } from "../src/features/workflow/schedule-label";

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
