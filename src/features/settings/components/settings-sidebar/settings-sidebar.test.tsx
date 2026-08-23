import { expect, test } from "bun:test";
import { renderToStaticMarkup } from "react-dom/server";
import { SettingsSidebar } from "./settings-sidebar";

test("includes License and Billing as a first-class settings destination", () => {
  const markup = renderToStaticMarkup(
    <SettingsSidebar
      activeSection="license-billing"
      onChange={() => {}}
      onBack={() => {}}
    />,
  );

  expect(markup).toContain("License &amp; Billing");
  expect(markup).toContain("Account");
  expect(markup).toContain('aria-current="page"');
});
