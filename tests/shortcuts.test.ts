import { describe, expect, test } from "bun:test";
import {
  DEFAULT_SHORTCUTS,
  findReservedShortcutConflict,
  findShortcutConflict,
  formatShortcut,
  shortcutFromKeyboardEvent,
} from "../src/features/settings/shortcuts";

function keyEvent(
  overrides: Partial<
    Pick<
      KeyboardEvent,
      "altKey" | "code" | "ctrlKey" | "key" | "metaKey" | "shiftKey"
    >
  >,
) {
  return {
    altKey: false,
    code: "KeyK",
    ctrlKey: false,
    key: "k",
    metaKey: false,
    shiftKey: false,
    ...overrides,
  };
}

describe("keyboard shortcut preferences", () => {
  test("records the platform primary modifier in a portable form", () => {
    expect(
      shortcutFromKeyboardEvent(
        keyEvent({ code: "Space", key: " ", metaKey: true, shiftKey: true }),
        true,
      ),
    ).toBe("CmdOrCtrl+Shift+Space");
    expect(
      shortcutFromKeyboardEvent(
        keyEvent({ code: "KeyK", ctrlKey: true, shiftKey: true }),
        false,
      ),
    ).toBe("CmdOrCtrl+Shift+K");
  });

  test("rejects unmodified typing keys but permits function keys", () => {
    expect(shortcutFromKeyboardEvent(keyEvent({}), true)).toBeNull();
    expect(
      shortcutFromKeyboardEvent(keyEvent({ code: "F8", key: "F8" }), true),
    ).toBe("F8");
  });

  test("formats accelerators for macOS and other desktop platforms", () => {
    expect(formatShortcut("CmdOrCtrl+Shift+Space", true)).toBe("⌘⇧Space");
    expect(formatShortcut("CmdOrCtrl+Shift+Space", false)).toBe(
      "Ctrl+Shift+Space",
    );
  });

  test("finds platform-equivalent conflicts", () => {
    expect(
      findShortcutConflict(
        DEFAULT_SHORTCUTS,
        "Cmd+N",
        "quickAccess",
        true,
      ),
    ).toBe("newWorkflow");
    expect(
      findShortcutConflict(
        DEFAULT_SHORTCUTS,
        "Alt+N",
        "quickAccess",
        true,
      ),
    ).toBeNull();
  });

  test("protects standard edit and application shortcuts", () => {
    expect(findReservedShortcutConflict("Cmd+C", true)).toBe("Copy");
    expect(findReservedShortcutConflict("CmdOrCtrl+Q", false)).toBe("Quit");
    expect(findReservedShortcutConflict("Alt+C", true)).toBeNull();
  });
});
