import { describe, expect, test } from "bun:test";
import {
  createHtmlReportPreview,
  extractHtmlReport,
} from "../src/features/workflow/html-report";

describe("HTML report preview", () => {
  test("recognizes full HTML documents but not ordinary output fragments", () => {
    expect(extractHtmlReport("<!doctype html><html><body>Report</body></html>"))
      .toBe("<!doctype html><html><body>Report</body></html>");
    expect(extractHtmlReport("<html lang=\"en\"><body>Report</body></html>"))
      .toBe("<html lang=\"en\"><body>Report</body></html>");
    expect(extractHtmlReport("Result: <strong>done</strong>")).toBeNull();
  });

  test("unwraps a Markdown fence around an HTML report", () => {
    expect(
      extractHtmlReport("```html\n<!doctype html><html><body>Report</body></html>\n```"),
    ).toBe("<!doctype html><html><body>Report</body></html>");
  });

  test("injects a restrictive preview policy without changing report content", () => {
    const preview = createHtmlReportPreview(
      "<!doctype html><html><head><style>body{color:red}</style></head><body>Report</body></html>",
    );

    expect(preview).toContain('Content-Security-Policy');
    expect(preview).toContain("default-src 'none'");
    expect(preview).toContain("style-src 'unsafe-inline'");
    expect(preview).toContain("<style>body{color:red}</style>");
    expect(preview).toContain("<body>Report</body>");
  });

  test("creates a head for documents that omit one", () => {
    const preview = createHtmlReportPreview("<html><body>Report</body></html>");
    expect(preview).toContain("<html><head>");
    expect(preview).toContain("</head><body>Report</body>");
  });
});
