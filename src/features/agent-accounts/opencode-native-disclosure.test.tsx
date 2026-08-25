import { renderToStaticMarkup } from "react-dom/server";
import { OpenCodeNativeDisclosure } from "./opencode-native-disclosure";

test("names the upstream billing boundary and blocked native capabilities", () => {
  const markup = renderToStaticMarkup(
    <OpenCodeNativeDisclosure connectAvailable={false} />,
  );
  expect(markup).toContain("OpenCode 1.18.23");
  expect(markup).toContain("actual upstream provider");
  expect(markup).toContain("billing owner");
  expect(markup).toContain("verified signed runtime");
  expect(markup).toContain("secure non-UI upstream credential entry flow");
  expect(markup).toContain("Alfred-owned tool execution");
  expect(markup).toContain("CLI harness remains separate");
  expect(markup).not.toContain("apiKey");
  expect(markup).not.toContain("token");
});

test("keeps upstream ownership visible after a future package gate passes", () => {
  const markup = renderToStaticMarkup(
    <OpenCodeNativeDisclosure connectAvailable />,
  );
  expect(markup).toContain("actual upstream provider");
  expect(markup).not.toContain("Native connect is unavailable");
});
