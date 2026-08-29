import { expect, test } from "bun:test";
import { renderToStaticMarkup } from "react-dom/server";
import { CursorNativeDisclosure } from "./cursor-native-disclosure";

test("states Cursor cloud billing, remote execution, and the blocked credential boundary", () => {
  const markup = renderToStaticMarkup(
    <CursorNativeDisclosure connectAvailable={false} />,
  );

  expect(markup).toContain("Cursor API key");
  expect(markup).toContain("billed through Cursor Cloud");
  expect(markup).toContain("executed remotely");
  expect(markup).toContain("remote repository");
  expect(markup).toContain("does not reuse local Cursor CLI credentials");
  expect(markup).toContain("secure API-key intake");
  expect(markup).not.toContain("credentialRef");
});

test("allows Alfred to open Cursor sign-in and dashboard URLs in the system browser", async () => {
  const capability = await Bun.file(
    new URL("../../../src-tauri/capabilities/default.json", import.meta.url),
  ).text();
  expect(capability).toContain('"https://cursor.com/*"');
  expect(capability).toContain('"https://www.cursor.com/*"');
  expect(capability).toContain('"https://authenticator.cursor.sh/*"');
});
