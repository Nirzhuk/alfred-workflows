import { describe, expect, test } from "bun:test";
import { formatQuickAccessNextRun } from "../src/features/quick-access/format-next-run";

describe("formatQuickAccessNextRun", () => {
  const now = Date.parse("2026-08-13T10:00:00Z");

  test("shows imminent runs as a concise relative time", () => {
    expect(
      formatQuickAccessNextRun("2026-08-13T10:14:30Z", now, "UTC"),
    ).toBe("In 15 min");
  });

  test("uses a local-time label for later runs", () => {
    expect(
      formatQuickAccessNextRun("2026-08-13T18:30:00Z", now, "UTC"),
    ).toContain("Today at");
  });

  test("handles missing and overdue schedules", () => {
    expect(formatQuickAccessNextRun(null, now, "UTC")).toBe("Not scheduled");
    expect(
      formatQuickAccessNextRun("2026-08-13T09:59:00Z", now, "UTC"),
    ).toBe("Due now");
  });
});
