import type { ReactNode } from "react";
import { Icon } from "../../../components/icon";
import { Modal, ModalHeader } from "../../../components/modal";
import { AppLogo } from "../app-logo";
import {
  TutorialWizard,
  type TutorialWizardStep,
} from "./tutorial-wizard";

type ConnectedAppTutorialLayoutProps = {
  providerId: string;
  providerName: string;
  title: ReactNode;
  titleId: string;
  description: ReactNode;
  badge: ReactNode;
  steps: TutorialWizardStep[];
  formLabel?: ReactNode;
  onClose: () => void;
  children: ReactNode;
};

export function ConnectedAppTutorialLayout({
  providerId,
  providerName,
  title,
  titleId,
  description,
  badge,
  steps,
  formLabel = "Then paste it into Alfred",
  onClose,
  children,
}: ConnectedAppTutorialLayoutProps) {
  const formHeadingId = titleId + "-form-heading";

  return (
    <Modal
      size="lg"
      className="connection-tutorial-modal connection-tutorial-split-modal"
      onClose={onClose}
      labelledBy={titleId}
      describedBy={titleId + "-description"}
    >
      <ModalHeader
        className="connection-tutorial-header"
        leading={
          <AppLogo providerId={providerId} providerName={providerName} size={40} />
        }
        title={title}
        titleId={titleId}
        titleAs="h2"
        description={description}
        descriptionId={titleId + "-description"}
        actions={
          <>
            <span className="connection-tutorial-badge">{badge}</span>
            <button
              type="button"
              className="connection-tutorial-close"
              aria-label="Close"
              onClick={onClose}
            >
              <Icon name="x" size={16} />
            </button>
          </>
        }
      />

      <div className="connection-tutorial-split-body">
        <section
          className="connection-tutorial-steps"
          aria-labelledby={titleId + "-steps-heading"}
        >
          <p
            id={titleId + "-steps-heading"}
            className="connection-tutorial-column-label"
          >
            Do this in {providerName}
          </p>
          <TutorialWizard
            ariaLabel={providerName + " setup steps"}
            steps={steps}
          />
        </section>

        <section
          className="connection-tutorial-form"
          aria-labelledby={formHeadingId}
        >
          <p id={formHeadingId} className="connection-tutorial-column-label">
            {formLabel}
          </p>
          {children}
        </section>
      </div>
    </Modal>
  );
}
