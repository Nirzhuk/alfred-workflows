import { expect, test } from "bun:test";
import { renderToStaticMarkup } from "react-dom/server";
import { GrokNativeDisclosure } from "./grok-native-disclosure";

test("separates xAI API billing from Grok subscriptions and CLI login", () => {
  const markup = renderToStaticMarkup(
    <GrokNativeDisclosure connectAvailable={false} />,
  );

  expect(markup).toContain("billed to your xAI API team");
  expect(markup).toContain("Grok subscription");
  expect(markup).toContain("Grok Build sign-in do not apply");
  expect(markup).toContain("secure API-key entry flow");
  expect(markup).not.toContain("token");
  expect(markup).not.toContain("credentialRef");
});
