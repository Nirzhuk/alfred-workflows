import type { ReactNode } from "react";

export type TutorialWizardStep = {
  label?: ReactNode;
  title: ReactNode;
  description?: ReactNode;
  note?: ReactNode;
  action?: ReactNode;
  status?: "complete" | "current" | "upcoming";
};

type TutorialWizardProps = {
  steps: TutorialWizardStep[];
  ariaLabel: string;
  className?: string;
};

export function TutorialWizard({
  steps,
  ariaLabel,
  className,
}: TutorialWizardProps) {
  return (
    <ol
      className={["tutorial-wizard", className].filter(Boolean).join(" ")}
      aria-label={ariaLabel}
    >
      {steps.map((step, index) => {
        const status = step.status ?? "static";
        return (
          <li
            className={`tutorial-wizard-step is-${status}`}
            key={`${index}-${String(step.title)}`}
            aria-current={status === "current" ? "step" : undefined}
          >
            <div className="tutorial-wizard-step-rail" aria-hidden="true">
              <span className="tutorial-wizard-step-number">
                {status === "complete" ? "✓" : index + 1}
              </span>
              {index < steps.length - 1 ? (
                <span className="tutorial-wizard-connector" />
              ) : null}
            </div>
            <div className="tutorial-wizard-step-content">
              <p className="tutorial-wizard-step-label">
                {step.label ?? `Step ${index + 1}`}
              </p>
              <div className="tutorial-wizard-step-heading">
                <h3>{step.title}</h3>
                {step.action ? (
                  <div className="tutorial-wizard-step-action">
                    {step.action}
                  </div>
                ) : null}
              </div>
              {step.description ? (
                <div className="tutorial-wizard-step-description">
                  {step.description}
                </div>
              ) : null}
              {step.note ? (
                <div className="tutorial-wizard-step-note">{step.note}</div>
              ) : null}
            </div>
          </li>
        );
      })}
    </ol>
  );
}
