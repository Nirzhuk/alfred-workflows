import { describe, expect, test } from "bun:test";

describe("Obsidian vault connection form", () => {
  test("uses a directory picker and does not persist the selected path", async () => {
    const source = await Bun.file(
      new URL(
        "../src/features/integrations/obsidian-vault-connect.tsx",
        import.meta.url,
      ),
    ).text();
    expect(source).toContain('directory: true');
    expect(source).toContain('multiple: false');
    expect(source).toContain('setVaultPath("")');
    expect(source).toContain("does not upload, index, edit, or watch");
    expect(source).toContain("Hidden folders and symlinks are ignored");
    expect(source).not.toContain("localStorage");
    expect(source).not.toContain("sessionStorage");
  });
});
