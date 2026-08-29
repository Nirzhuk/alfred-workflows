import { expect, test } from "bun:test";
import { renderToStaticMarkup } from "react-dom/server";
import { NativeHostApprovalContent } from "./native-host-approval";

test("renders once, always, and reject without leaking request internals as secrets", () => {
  const markup = renderToStaticMarkup(
    <NativeHostApprovalContent
      prompt={{
        requestId: "approval_fixture",
        permission: "bash",
        patterns: ["src/**"],
        alwaysPatterns: [],
      }}
      onDecision={() => {}}
    />,
  );
  expect(markup).toContain("Allow this tool?");
  expect(markup).toContain("Once");
  expect(markup).toContain("Always");
  expect(markup).toContain("Reject");
  expect(markup).toContain("bash");
  expect(markup).not.toContain("apiKey");
  expect(markup).not.toContain("credential");
});
