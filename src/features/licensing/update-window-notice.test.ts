import { describe, expect, test } from "bun:test";
import {
  dismissUpdateWindowNotice,
  readUpdateWindowNoticeDismissed,
  UPDATE_WINDOW_NOTICE_BODY,
  UPDATE_WINDOW_NOTICE_KEY,
  UPDATE_WINDOW_NOTICE_TITLE,
} from "./update-window-notice";
function memoryStorage(initial: Record<string, string> = {}) {
  const values = new Map(Object.entries(initial));
  return {
    getItem: (key: string) => values.get(key) ?? null,
    setItem: (key: string, value: string) => {
      values.set(key, value);
    },
  };
}

function failingStorage() {
  return {
    getItem: () => {
      throw new Error("storage unavailable");
    },
    setItem: () => {
      throw new Error("storage unavailable");
    },
  };
}

describe("update window notice", () => {
  test("is owed once and only once per build", () => {
    const storage = memoryStorage();
    expect(!readUpdateWindowNoticeDismissed(storage)).toBe(true);

    dismissUpdateWindowNotice(storage);
    expect(readUpdateWindowNoticeDismissed(storage)).toBe(true);
    expect(!readUpdateWindowNoticeDismissed(storage)).toBe(false);
  });

  test("reads an existing dismissal without re-showing", () => {
    const storage = memoryStorage({ [UPDATE_WINDOW_NOTICE_KEY]: "1" });
    expect(!readUpdateWindowNoticeDismissed(storage)).toBe(false);
  });

  test("any other stored value is not a dismissal", () => {
    const storage = memoryStorage({ [UPDATE_WINDOW_NOTICE_KEY]: "true" });
    expect(!readUpdateWindowNoticeDismissed(storage)).toBe(true);
  });

  test("a broken store never blocks the app", () => {
    expect(!readUpdateWindowNoticeDismissed(failingStorage())).toBe(true);
    // Dismissing into a broken store fails quietly; the notice may show
    // again rather than the app crashing.
    expect(() =>
      dismissUpdateWindowNotice(failingStorage()),
    ).not.toThrow();
  });

  test("the drafted copy stays honest about what lapsing does", () => {
    // Plan 007's promise, in draft form pending owner approval: nothing the
    // customer paid for is taken away, data is untouched, downloading newer
    // releases is never blocked.
    expect(UPDATE_WINDOW_NOTICE_TITLE).toContain("update window");
    expect(UPDATE_WINDOW_NOTICE_TITLE.toLowerCase()).not.toContain("expired");
    expect(UPDATE_WINDOW_NOTICE_TITLE.toLowerCase()).not.toContain("license");

    const body = UPDATE_WINDOW_NOTICE_BODY;
    expect(body).toContain("local data stay intact");
    expect(body).toContain("every feature you paid for keeps working");
    expect(body).not.toContain("lost");
    expect(body).not.toContain("upgrade now");
    expect(body).not.toContain("limited time");

    // No dark pattern: a deadline pressure line would be one.
    expect(body.toLowerCase()).not.toContain("today");
    expect(body.toLowerCase()).not.toMatch(/\bdays? left\b/);
  });
});
