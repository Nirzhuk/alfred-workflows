export const TELEGRAM_SETUP_PROGRESS_STEPS = [
  "Create bot",
  "Add token",
  "Open link",
  "Send test",
];

export type TelegramStepStatus = "done" | "current" | "upcoming";

export function telegramStepStatus(
  index: number,
  activeIndex: number,
): TelegramStepStatus {
  if (index < activeIndex) return "done";
  if (index === activeIndex) return "current";
  return "upcoming";
}

export function TelegramSetupProgress({
  pairingStarted,
}: {
  pairingStarted: boolean;
}) {
  const activeIndex = pairingStarted ? 2 : 1;

  return (
    <ol className="telegram-progress" aria-label="Telegram setup progress">
      {TELEGRAM_SETUP_PROGRESS_STEPS.map((label, index) => {
        const status = telegramStepStatus(index, activeIndex);
        return (
          <li key={label} className={`telegram-progress-step is-${status}`}>
            <span className="telegram-progress-node" aria-hidden>
              {status === "done" ? (
                <svg viewBox="0 0 16 16" fill="none">
                  <path
                    d="M4 8.5l2.5 2.5L12 5.5"
                    stroke="currentColor"
                    strokeWidth="1.6"
                    strokeLinecap="round"
                    strokeLinejoin="round"
                  />
                </svg>
              ) : (
                index + 1
              )}
            </span>
            <span className="telegram-progress-label">{label}</span>
          </li>
        );
      })}
    </ol>
  );
}
