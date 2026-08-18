import { expect, test } from "bun:test";
import { renderToStaticMarkup } from "react-dom/server";
import { TutorialWizard } from "./tutorial-wizard";

test("renders reusable horizontal tutorial steps with actions", () => {
  const markup = renderToStaticMarkup(
    <TutorialWizard
      ariaLabel="Example setup steps"
      steps={[
        {
          title: "Create an app",
          description: <p>Choose the target workspace.</p>,
          action: <button type="button">Open provider</button>,
        },
        {
          title: "Paste the token",
          status: "upcoming",
        },
      ]}
    />,
  );

  expect(markup).toContain('aria-label="Example setup steps"');
  expect(markup).toContain("tutorial-wizard-step is-static");
  expect(markup).toContain("tutorial-wizard-step is-upcoming");
  expect(markup).toContain("Create an app");
  expect(markup).toContain("Choose the target workspace.");
  expect(markup).toContain("Open provider");
  expect(markup).toContain("tutorial-wizard-connector");
});

test("renders completed steps with a check mark", () => {
  const markup = renderToStaticMarkup(
    <TutorialWizard
      ariaLabel="Progress"
      steps={[{ title: "Done", status: "complete" }]}
    />,
  );

  expect(markup).toContain("tutorial-wizard-step is-complete");
  expect(markup).toContain("✓");
});
