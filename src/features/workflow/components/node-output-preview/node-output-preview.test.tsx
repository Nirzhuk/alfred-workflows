import { expect, test } from "bun:test";
import { previewText } from "./node-output-preview";

test("shows only the first paragraph, leaving JSON receipts out of the compact card", () => {
  const output =
    'Telegram accepted the notification\n\n{\n  "acceptedAt": "2026-08-17T16:26:57Z",\n  "messageId": 6\n}';
  expect(previewText(output)).toBe("Telegram accepted the notification");
});

test("flattens a single-paragraph output onto one line and truncates at max", () => {
  expect(previewText("Hi!\nHow can I help you today?")).toBe(
    "Hi! How can I help you today?",
  );
  expect(previewText("a".repeat(200), 10)).toBe(`${"a".repeat(10)}…`);
});
