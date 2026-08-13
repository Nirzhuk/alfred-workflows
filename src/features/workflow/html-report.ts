const HTML_FENCE = /^```(?:html)?[ \t]*\r?\n([\s\S]*?)\r?\n```[ \t]*$/i;
const HTML_DOCUMENT_START =
  /^(?:<!doctype\s+html(?:\s[^>]*)?>\s*)?<html(?:\s[^>]*)?>/i;

const PREVIEW_CSP =
  "<meta http-equiv=\"Content-Security-Policy\" content=\"default-src 'none'; style-src 'unsafe-inline'; img-src data: blob:; font-src data:; media-src data: blob:; base-uri 'none'; form-action 'none'\">";

/**
 * Returns a full HTML document, unwrapping a single Markdown fence when an
 * agent added one despite being asked for raw HTML. Ordinary HTML fragments
 * are deliberately left as text.
 */
export function extractHtmlReport(value: string): string | null {
  const trimmed = value.trim();
  const fenced = trimmed.match(HTML_FENCE);
  const candidate = (fenced?.[1] ?? trimmed).trim();
  return HTML_DOCUMENT_START.test(candidate) ? candidate : null;
}

/**
 * Adds a restrictive policy to the document used by the sandboxed preview.
 * This leaves inline report styling intact while blocking scripts and remote
 * resources. The stored output itself is never changed.
 */
export function createHtmlReportPreview(value: string): string | null {
  const html = extractHtmlReport(value);
  if (!html) return null;

  const head = /<head(?:\s[^>]*)?>/i.exec(html);
  if (head?.index !== undefined) {
    const insertionPoint = head.index + head[0].length;
    return `${html.slice(0, insertionPoint)}${PREVIEW_CSP}${html.slice(insertionPoint)}`;
  }

  const root = /<html(?:\s[^>]*)?>/i.exec(html);
  if (root?.index !== undefined) {
    const insertionPoint = root.index + root[0].length;
    return `${html.slice(0, insertionPoint)}<head>${PREVIEW_CSP}</head>${html.slice(insertionPoint)}`;
  }

  return null;
}
