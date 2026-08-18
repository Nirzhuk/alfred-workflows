import { expect, test } from "bun:test";
import { renderToStaticMarkup } from "react-dom/server";
import {
  TELEGRAM_SETUP_PROGRESS_STEPS,
  TelegramSetupProgress,
  telegramStepStatus,
} from "./telegram-setup-progress";

test("before pairing, token entry is current and later steps are upcoming", () => {
  const markup = renderToStaticMarkup(
    <TelegramSetupProgress pairingStarted={false} />,
  );
  expect(markup).toContain("telegram-progress-step is-done");
  expect(markup).toContain("telegram-progress-step is-current");
  expect(markup).toContain("telegram-progress-step is-upcoming");
  for (const label of TELEGRAM_SETUP_PROGRESS_STEPS) {
    expect(markup).toContain(label);
  }
});

test("once pairing starts, bot and token steps are marked done", () => {
  expect(telegramStepStatus(0, 2)).toBe("done");
  expect(telegramStepStatus(1, 2)).toBe("done");
  expect(telegramStepStatus(2, 2)).toBe("current");
  expect(telegramStepStatus(3, 2)).toBe("upcoming");
});
