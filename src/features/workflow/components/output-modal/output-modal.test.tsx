import { expect, test } from "bun:test";
import { splitSummaryAndJson } from "./output-modal";

test("splits a summary-then-receipt body into prose and JSON", () => {
  const body =
    'Telegram accepted the notification\n\n{\n  "acceptedAt": "2026-08-17T16:26:57Z",\n  "messageId": 6\n}';
  expect(splitSummaryAndJson(body)).toEqual({
    summary: "Telegram accepted the notification",
    json: '{\n  "acceptedAt": "2026-08-17T16:26:57Z",\n  "messageId": 6\n}',
  });
});

test("leaves plain agent text and non-JSON bodies alone", () => {
  expect(splitSummaryAndJson("Hi! How can I help you today?")).toBeNull();
  expect(splitSummaryAndJson("Two lines\nof one paragraph")).toBeNull();
  expect(splitSummaryAndJson("Not JSON\n\nstill not json")).toBeNull();
  expect(splitSummaryAndJson("Broken\n\n{not valid json}")).toBeNull();
});
