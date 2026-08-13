import { describe, expect, test } from "bun:test";
import {
  defaultOutputNodeData,
  normalizeOutputNodeData,
} from "../src/features/workflow/types";

describe("HTML report Output option", () => {
  test("is opt-in for new and existing Output nodes", () => {
    expect(defaultOutputNodeData().htmlReport).toBe(false);
    expect(normalizeOutputNodeData({ label: "Legacy output" }).htmlReport).toBe(
      false,
    );
  });

  test("preserves an enabled HTML report option", () => {
    expect(
      normalizeOutputNodeData({ label: "Report", htmlReport: true }).htmlReport,
    ).toBe(true);
  });
});
